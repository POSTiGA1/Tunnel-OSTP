use crate::config::{ClientConfig, OutboundConfig};
use std::collections::HashMap;

pub struct Balancer {
    outbounds: HashMap<String, OutboundConfig>,
}

impl Balancer {
    pub fn new(config: &ClientConfig) -> Self {
        let mut outbounds = HashMap::new();
        for outbound in &config.outbounds {
            let tag = match outbound {
                OutboundConfig::Selector { tag, .. } => tag,
                OutboundConfig::Urltest { tag, .. } => tag,
                OutboundConfig::Ostp { tag, .. } => tag,
                OutboundConfig::Direct { tag } => tag,
                OutboundConfig::Socks { tag, .. } => tag,
                OutboundConfig::Block { tag } => tag,
            };
            outbounds.insert(tag.clone(), outbound.clone());
        }

        Self { outbounds }
    }

    /// Resolves an outbound tag into a concrete, non-group outbound tag.
    /// E.g. "proxy-group" -> "server-helsinki"
    pub fn resolve_outbound(&self, tag: &str) -> String {
        // Prevent infinite loops if groups point to groups
        let mut current_tag = tag.to_string();
        for _ in 0..10 {
            if let Some(outbound) = self.outbounds.get(&current_tag) {
                match outbound {
                    OutboundConfig::Selector { outbounds, default, .. } => {
                        current_tag = if let Some(def) = default {
                            def.clone()
                        } else {
                            outbounds.first().cloned().unwrap_or_else(|| "direct".to_string())
                        };
                    }
                    OutboundConfig::Urltest { outbounds, .. } => {
                        // TODO: Implement background ping worker to find the fastest node.
                        // For now, act as a fallback by taking the first available node.
                        current_tag = outbounds.first().cloned().unwrap_or_else(|| "direct".to_string());
                    }
                    _ => {
                        // It's a concrete physical outbound (ostp, direct, block)
                        return current_tag;
                    }
                }
            } else {
                // Outbound not found, fallback to direct
                return "direct".to_string();
            }
        }
        "direct".to_string() // Max depth reached
    }

    /// Fetches the config for a concrete outbound
    pub fn get_concrete_outbound(&self, tag: &str) -> Option<&OutboundConfig> {
        let resolved_tag = self.resolve_outbound(tag);
        tracing::debug!("Balancer: tag '{}' resolved to '{}'", tag, resolved_tag);
        self.outbounds.get(&resolved_tag)
    }
}
