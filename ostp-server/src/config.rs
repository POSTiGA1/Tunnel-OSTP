use serde::{Deserialize, Serialize};
use crate::{api::ApiConfig, fallback::FallbackConfig, outbound::OutboundConfig, dns::DnsConfig};

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerInbound {
    Ostp {
        tag: String,
        listen: String,
        port: u16,
        #[serde(default)]
        users: Vec<UserConfig>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fallback: Option<FallbackConfig>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transport: Option<TransportConfigRaw>,
    },
    Api {
        tag: String,
        listen: String,
        port: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        webpath: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        username: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password_hash: Option<String>,
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum UserConfig {
    KeyOnly(String),
    Detailed {
        #[serde(rename = "key")]
        access_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit_bytes: Option<u64>,
    },
}

impl UserConfig {
    pub fn key(&self) -> String {
        match self {
            UserConfig::KeyOnly(k) => k.clone(),
            UserConfig::Detailed { access_key, .. } => access_key.clone(),
        }
    }
    pub fn name(&self) -> Option<String> {
        match self {
            UserConfig::KeyOnly(_) => None,
            UserConfig::Detailed { name, .. } => name.clone(),
        }
    }
    pub fn limit(&self) -> Option<u64> {
        match self {
            UserConfig::KeyOnly(_) => None,
            UserConfig::Detailed { limit_bytes, .. } => *limit_bytes,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TransportConfigRaw {
    pub mode: Option<String>,
    pub stealth_sni: Option<String>,
    pub wss: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerOutbound {
    Socks {
        tag: String,
        server: String,
        port: u16,
    },
    Direct {
        tag: String,
    },
    Block {
        tag: String,
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerRouting {
    #[serde(default)]
    pub rules: Vec<ServerRoutingRule>,
    pub default_outbound: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerRoutingRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_suffix: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_cidr: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    pub outbound: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ModularServerConfig {
    #[serde(default)]
    pub inbounds: Vec<ServerInbound>,
    #[serde(default)]
    pub outbounds: Vec<ServerOutbound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<ServerRouting>,
    
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns: Option<DnsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_key: Option<String>,
    
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_transport: Option<DnsTransportConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DnsTransportConfig {
    #[serde(default)]
    pub enabled: bool,
    pub listen: String,
    pub domain: String,
    pub pubkey: String,
    pub privkey: String,
}
