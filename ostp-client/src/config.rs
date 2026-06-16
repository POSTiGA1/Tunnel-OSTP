use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default)]
    pub inbounds: Vec<InboundConfig>,
    #[serde(default)]
    pub outbounds: Vec<OutboundConfig>,
    #[serde(default)]
    pub routing: RoutingConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gui: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self { level: default_log_level() }
    }
}

fn default_log_level() -> String { "info".to_string() }
fn default_true() -> bool { true }
pub fn default_mtu() -> usize { 1140 }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboundConfig {
    Tun {
        tag: String,
        #[serde(default = "default_true")]
        auto_route: bool,
        #[serde(default = "default_mtu")]
        mtu: usize,
    },
    LocalProxy {
        tag: String,
        protocol: String, // "socks" or "http"
        listen: String,
        port: u16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundConfig {
    Selector {
        tag: String,
        outbounds: Vec<String>,
        default: Option<String>,
    },
    Urltest {
        tag: String,
        outbounds: Vec<String>,
        url: Option<String>,
        interval: Option<String>,
    },
    Ostp {
        tag: String,
        server: String,
        port: u16,
        access_key: String,
        #[serde(default)]
        transport: TransportConfig,
        #[serde(default)]
        multiplex: MultiplexConfig,
    },
    Direct {
        tag: String,
    },
    Socks {
        tag: String,
        server: String,
        port: u16,
    },
    Block {
        tag: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    #[serde(default = "default_transport_mode")]
    pub r#type: String, // "udp" or "uot"
}

fn default_transport_mode() -> String { "udp".to_string() }

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            r#type: default_transport_mode(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiplexConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_mux_sessions")]
    pub sessions: usize,
}

fn default_mux_sessions() -> usize { 1 }

impl Default for MultiplexConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sessions: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoutingConfig {
    #[serde(default)]
    pub rules: Vec<RoutingRule>,
    #[serde(default)]
    pub default_outbound: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_suffix: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_cidr: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_name: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbound_tag: Option<Vec<String>>,
    pub outbound: String,
}

impl ClientConfig {
    /// Hot-reload from `config.json` placed next to the running binary.
    /// Returns a new `ClientConfig` built from the JSON format.
    pub fn reload_from_json_near_binary() -> Result<Self> {
        let exe = std::env::current_exe().context("cannot resolve binary path")?;
        let dir = exe.parent().context("cannot resolve binary directory")?;
        let path = dir.join("config.json");

        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut stripped = json_comments::StripComments::new(raw.as_bytes());
        let config: ClientConfig = serde_json::from_reader(&mut stripped)
            .with_context(|| format!("failed to parse {}", path.display()))?;

        Ok(config)
    }
}
