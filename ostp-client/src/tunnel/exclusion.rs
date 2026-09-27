use crate::config::ExclusionConfig;
use std::time::Duration;
use tokio::time::timeout;

#[derive(Clone)]
pub struct ExclusionMatcher {
    pub domain_suffix: Vec<String>,
    pub cidrs: Vec<Cidr>,
    pub processes: Vec<String>,
    pub physical_if_index: Option<u32>,
    pub physical_if_name: Option<String>,
    /// Everything this matcher is asked about goes direct: set for one
    /// connection whose program is excluded.
    pub bypass_all: bool,
}

impl ExclusionMatcher {
    pub fn new(
        exclusions: &ExclusionConfig,
        physical_if_index: Option<u32>,
        physical_if_name: Option<String>,
    ) -> Self {
        let mut cidrs = Vec::new();
        for ip in &exclusions.ips {
            if let Some(cidr) = parse_cidr(ip) {
                cidrs.push(cidr);
            }
        }

        let processes = exclusions.processes.iter()
            .map(|p| p.trim().to_lowercase())
            .filter(|p| !p.is_empty())
            .collect();

        Self {
            domain_suffix: exclusions
                .domains
                .iter()
                .map(|d| normalize_domain(d.trim_start_matches(['.', '*'])))
                .filter(|d| !d.is_empty())
                .collect(),
            cidrs,
            processes,
            physical_if_index,
            physical_if_name,
            bypass_all: false,
        }
    }

    pub async fn should_bypass_target(&self, host: &str, port: u16, timeout_value: Duration) -> bool {
        if self.bypass_all {
            return true;
        }
        if self.match_domain(host) {
            return true;
        }

        if self.cidrs.is_empty() {
            return false;
        }

        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            return self.match_ip(&ip);
        }

        let lookup_target = (host.to_string(), port);
        match timeout(timeout_value, tokio::net::lookup_host(lookup_target)).await {
            Ok(Ok(addrs)) => addrs.into_iter().any(|addr| self.match_ip(&addr.ip())),
            _ => false,
        }
    }

    pub fn match_domain(&self, host: &str) -> bool {
        if self.domain_suffix.is_empty() {
            return false;
        }
        let host = normalize_domain(host);
        self.domain_suffix.iter().any(|suffix| {
            host == *suffix || host.ends_with(&format!(".{suffix}"))
        })
    }

    pub fn match_ip(&self, ip: &std::net::IpAddr) -> bool {
        self.cidrs.iter().any(|cidr| cidr.contains(ip))
    }

    /// The name of the program owning the local socket `local`, when that
    /// program is excluded. Looks the owner up only when process exclusions
    /// are configured, since the lookup reads the system's socket tables.
    pub async fn excluded_process(
        &self,
        proto: crate::tunnel::process_lookup::Proto,
        local: std::net::SocketAddr,
    ) -> Option<String> {
        if self.processes.is_empty() {
            return None;
        }
        let name = tokio::task::spawn_blocking(move || {
            crate::tunnel::process_lookup::process_owning(proto, local)
        })
        .await
        .ok()
        .flatten();
        match name {
            Some(name) if self.match_process(&name) => Some(name),
            Some(name) => {
                tracing::debug!("{local} belongs to {name}, not excluded");
                None
            }
            None => {
                tracing::debug!("no process found for {local}");
                None
            }
        }
    }

    pub fn match_process(&self, process_name: &str) -> bool {
        if self.processes.is_empty() {
            return false;
        }
        let p = process_name.to_lowercase();
        self.processes.iter().any(|ex| p.contains(ex))
    }
}

/// The form a domain has on the wire: lower case, no trailing dot, and an
/// internationalized name (`пример.рф`) in punycode (`xn--e1afmkfd.xn--p1ai`),
/// which is what DNS, SNI and SOCKS requests carry. A string that is not a
/// valid domain is only lower-cased, so it still matches itself.
pub fn normalize_domain(domain: &str) -> String {
    let domain = domain.trim().trim_end_matches('.');
    if domain.is_ascii() {
        return domain.to_ascii_lowercase();
    }
    idna::domain_to_ascii(domain).unwrap_or_else(|_| domain.to_lowercase())
}

#[derive(Clone)]
pub enum Cidr {
    V4(u32, u8),
    V6(u128, u8),
}

impl Cidr {
    pub fn contains(&self, ip: &std::net::IpAddr) -> bool {
        match (self, ip) {
            (Cidr::V4(net, bits), std::net::IpAddr::V4(addr)) => {
                let mask = if *bits == 0 { 0 } else { u32::MAX << (32 - bits) };
                let ip = u32::from_be_bytes(addr.octets());
                (ip & mask) == (*net & mask)
            }
            (Cidr::V6(net, bits), std::net::IpAddr::V6(addr)) => {
                let mask = if *bits == 0 { 0 } else { u128::MAX << (128 - bits) };
                let ip = u128::from_be_bytes(addr.octets());
                (ip & mask) == (*net & mask)
            }
            _ => false,
        }
    }
}

pub fn parse_cidr(s: &str) -> Option<Cidr> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.is_empty() || parts.len() > 2 {
        return None;
    }
    if let Ok(ip) = parts[0].parse::<std::net::IpAddr>() {
        let bits = if parts.len() == 2 {
            parts[1].parse::<u8>().ok()?
        } else {
            match ip {
                std::net::IpAddr::V4(_) => 32,
                std::net::IpAddr::V6(_) => 128,
            }
        };
        match ip {
            std::net::IpAddr::V4(v4) => Some(Cidr::V4(u32::from_be_bytes(v4.octets()), bits)),
            std::net::IpAddr::V6(v6) => Some(Cidr::V6(u128::from_be_bytes(v6.octets()), bits)),
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher(domains: &[&str]) -> ExclusionMatcher {
        let cfg = ExclusionConfig {
            domains: domains.iter().map(|d| d.to_string()).collect(),
            ..Default::default()
        };
        ExclusionMatcher::new(&cfg, None, None)
    }

    #[test]
    fn internationalized_domains_match_their_punycode() {
        let m = matcher(&["пример.рф", ".Госуслуги.РФ"]);
        assert!(m.match_domain("xn--e1afmkfd.xn--p1ai"));
        assert!(m.match_domain("www.xn--e1afmkfd.xn--p1ai."));
        assert!(m.match_domain("пример.рф"));
        assert!(m.match_domain("lk.xn--c1aapkosapc.xn--p1ai"));
        assert!(!m.match_domain("xn--p1ai"));
    }

    #[test]
    fn ascii_domains_match_suffixes_only_at_a_label() {
        let m = matcher(&["Example.com", "*.test.org"]);
        assert!(m.match_domain("example.com"));
        assert!(m.match_domain("a.b.EXAMPLE.com."));
        assert!(!m.match_domain("badexample.com"));
        assert!(m.match_domain("x.test.org"));
    }

    #[tokio::test]
    async fn an_excluded_program_bypasses_everything() {
        let mut m = matcher(&[]);
        let t = Duration::from_millis(10);
        assert!(!m.should_bypass_target("example.com", 443, t).await);
        m.bypass_all = true;
        assert!(m.should_bypass_target("example.com", 443, t).await);
    }
}
