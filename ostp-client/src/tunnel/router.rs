use std::net::IpAddr;
use crate::config::{RoutingConfig, RoutingRule};

#[derive(Debug, Clone)]
pub struct Session {
    pub inbound_tag: String,
    pub source_ip: Option<IpAddr>,
    pub destination_ip: Option<IpAddr>,
    pub destination_port: u16,
    pub protocol: String, // "tcp" or "udp"
    pub sni: Option<String>,
    pub process_name: Option<String>,
}

pub struct Router {
    config: RoutingConfig,
}

impl Router {
    pub fn new(config: RoutingConfig) -> Self {
        Self { config }
    }

    /// Evaluates the session against routing rules and returns the outbound tag
    pub fn route(&self, session: &Session) -> String {
        for rule in &self.config.rules {
            if self.match_rule(rule, session) {
                return rule.outbound.clone();
            }
        }
        self.config.default_outbound.clone()
    }

    fn match_rule(&self, rule: &RoutingRule, session: &Session) -> bool {
        // All specified conditions in a rule must match (AND logic)
        let mut matched_any_condition = false;

        // 1. Inbound Tag match
        if let Some(inbounds) = &rule.inbound_tag {
            if !inbounds.iter().any(|tag| tag == &session.inbound_tag) {
                return false;
            }
            matched_any_condition = true;
        }

        // 2. Domain / SNI match
        if let Some(domains) = &rule.domain_suffix {
            let mut domain_match = false;
            if let Some(sni) = &session.sni {
                let sni = sni.to_lowercase();
                domain_match = domains.iter().any(|d| {
                    let d = d.to_lowercase();
                    sni == d || sni.ends_with(&format!(".{}", d))
                });
            }
            if !domain_match {
                return false;
            }
            matched_any_condition = true;
        }

        // 3. Process match
        if let Some(processes) = &rule.process_name {
            let mut proc_match = false;
            if let Some(proc) = &session.process_name {
                let proc = proc.to_lowercase();
                proc_match = processes.iter().any(|p| proc.contains(&p.to_lowercase()));
            }
            if !proc_match {
                return false;
            }
            matched_any_condition = true;
        }

        // 4. IP CIDR match
        if let Some(cidrs) = &rule.ip_cidr {
            let mut ip_match = false;
            if let Some(dst_ip) = session.destination_ip {
                ip_match = cidrs.iter().any(|cidr| {
                    match ipnet::IpNet::from_str(cidr) {
                        Ok(net) => net.contains(&dst_ip),
                        Err(_) => {
                            // fallback to exact ip match if not a valid CIDR
                            if let Ok(ip) = cidr.parse::<IpAddr>() {
                                ip == dst_ip
                            } else {
                                false
                            }
                        }
                    }
                });
            }
            if !ip_match {
                return false;
            }
            matched_any_condition = true;
        }

        // A rule must have at least one condition to match
        matched_any_condition
    }
}

use std::str::FromStr;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router() {
        let rules = vec![
            RoutingRule {
                domain_suffix: Some(vec!["vk.com".to_string()]),
                ip_cidr: None,
                process_name: None,
                inbound_tag: None,
                outbound: "direct".to_string(),
            },
            RoutingRule {
                domain_suffix: None,
                ip_cidr: None,
                process_name: Some(vec!["telegram.exe".to_string()]),
                inbound_tag: None,
                outbound: "proxy-group".to_string(),
            },
        ];

        let config = RoutingConfig {
            rules,
            default_outbound: "proxy-group".to_string(),
        };

        let router = Router::new(config);

        let mut session = Session {
            inbound_tag: "tun-in".to_string(),
            source_ip: None,
            destination_ip: None,
            destination_port: 443,
            protocol: "tcp".to_string(),
            sni: Some("api.vk.com".to_string()),
            process_name: None,
        };

        assert_eq!(router.route(&session), "direct");

        session.sni = None;
        session.process_name = Some("C:\\App\\Telegram.exe".to_string());
        assert_eq!(router.route(&session), "proxy-group");

        session.process_name = Some("chrome.exe".to_string());
        assert_eq!(router.route(&session), "proxy-group"); // fallback
    }
}
