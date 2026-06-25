use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fd: Option<i32>,
    },
    LocalProxy {
        tag: String,
        protocol: String, // "socks" or "http"
        listen: String,
        port: u16,
        #[serde(default)]
        set_system_proxy: bool,
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
    pub r#type: String, // "udp", "uot", or "dns"
    
    // Settings for DNS transport
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pubkey: Option<String>,
    
    // Obfuscation
    #[serde(default = "default_false")]
    pub tcp_fragmentation: bool,
}

fn default_false() -> bool { false }

fn default_transport_mode() -> String { "udp".to_string() }

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            r#type: default_transport_mode(),
            domain: None,
            resolver: None,
            pubkey: None,
            tcp_fragmentation: false,
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
        let raw_json: serde_json::Value = serde_json::from_reader(&mut stripped)
            .with_context(|| format!("failed to parse JSON from {}", path.display()))?;

        let (migrated_json, was_migrated) = Self::migrate_json(raw_json);
        if was_migrated {
            tracing::warn!(
                "Config at {} is in an outdated format. Run 'ostp --migrate' to upgrade it.",
                path.display()
            );
        }

        let config: ClientConfig = serde_json::from_value(migrated_json)
            .with_context(|| format!("failed to deserialize config from {}", path.display()))?;

        Ok(config)
    }

    /// Migrates old monolithic JSON to the new modular format.
    /// Returns the migrated JSON value and a boolean indicating if a migration occurred.
    pub fn migrate_json(json: serde_json::Value) -> (serde_json::Value, bool) {
        // Consider the config already migrated if:
        // 1. Version matches exactly, OR
        // 2. The JSON already has the new modular format (inbounds + outbounds arrays)
        let has_version = json.get("version").and_then(|v| v.as_str()) == Some(env!("CARGO_PKG_VERSION"));
        let has_new_format = json.get("inbounds").and_then(|v| v.as_array()).is_some()
            && json.get("outbounds").and_then(|v| v.as_array()).is_some();
        
        if has_version || has_new_format {
            // If format is already new but version is old, just bump the version
            if has_new_format && !has_version {
                let mut updated = json.clone();
                updated["version"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
                return (updated, false);
            }
            return (json, false);
        }

        // Needs migration
        let mut new_json = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
        });

        // 1. Log level
        let log_level = if let Some(ll) = json.get("log_level") {
            ll.clone()
        } else if let Some(d) = json.get("debug") {
            if d.as_bool().unwrap_or(false) { serde_json::json!("debug") } else { serde_json::json!("info") }
        } else {
            serde_json::json!("info")
        };
        new_json["log"] = serde_json::json!({ "level": log_level });

        // 2. Inbounds
        let mut inbounds = Vec::new();
        
        if let Some(tun) = json.get("tun") {
            if tun.get("enable").and_then(|v| v.as_bool()).unwrap_or(false) {
                inbounds.push(serde_json::json!({
                    "type": "tun",
                    "tag": "tun-in",
                    "auto_route": true,
                    "mtu": 1140
                }));
            }
        }

        let socks_bind = json.get("socks5_bind").and_then(|v| v.as_str()).unwrap_or("127.0.0.1:1088");
        let parts: Vec<&str> = socks_bind.split(':').collect();
        let listen = parts.get(0).unwrap_or(&"127.0.0.1");
        let port = parts.get(1).unwrap_or(&"1088").parse::<u16>().unwrap_or(1088);
        
        inbounds.push(serde_json::json!({
            "type": "local_proxy",
            "tag": "socks-in",
            "protocol": "socks",
            "listen": listen,
            "port": port
        }));

        new_json["inbounds"] = serde_json::Value::Array(inbounds);

        // 3. Outbounds
        let mut outbounds = Vec::new();
        
        let server_full = json.get("server").and_then(|v| v.as_str())
            .or_else(|| json.get("ostp").and_then(|o| o.get("server_addr")).and_then(|v| v.as_str()))
            .unwrap_or("127.0.0.1:50000");
            
        let access_key = json.get("access_key").and_then(|v| v.as_str())
            .or_else(|| json.get("ostp").and_then(|o| o.get("access_key")).and_then(|v| v.as_str()))
            .unwrap_or("");
            
        let transport_type = json.get("transport").and_then(|t| t.get("mode").or(t.get("type"))).and_then(|v| v.as_str()).unwrap_or("udp");
        let mux_enabled = json.get("mux").and_then(|m| m.get("enabled")).and_then(|v| v.as_bool()).unwrap_or(false);
        let mux_sessions = json.get("mux").and_then(|m| m.get("sessions")).and_then(|v| v.as_u64()).unwrap_or(1);

        let servers: Vec<&str> = server_full.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        let mut ostp_tags = Vec::new();

        for (i, server_str) in servers.iter().enumerate() {
            let server_parts: Vec<&str> = server_str.split(':').collect();
            let server_host = server_parts.get(0).unwrap_or(&"127.0.0.1");
            let server_port = server_parts.get(1).unwrap_or(&"50000").parse::<u16>().unwrap_or(50000);
            
            let tag = if servers.len() > 1 { format!("proxy-{}", i) } else { "proxy".to_string() };
            ostp_tags.push(tag.clone());

            outbounds.push(serde_json::json!({
                "type": "ostp",
                "tag": tag,
                "server": server_host,
                "port": server_port,
                "access_key": access_key,
                "transport": {
                    "type": transport_type
                },
                "multiplex": {
                    "enabled": mux_enabled,
                    "sessions": mux_sessions
                }
            }));
        }

        if servers.len() > 1 {
            outbounds.push(serde_json::json!({
                "type": "urltest",
                "tag": "proxy",
                "outbounds": ostp_tags,
                "url": "http://cp.cloudflare.com",
                "interval": "3m"
            }));
        }

        outbounds.push(serde_json::json!({
            "type": "direct",
            "tag": "direct"
        }));

        outbounds.push(serde_json::json!({
            "type": "block",
            "tag": "block"
        }));

        new_json["outbounds"] = serde_json::Value::Array(outbounds);

        // 4. Routing
        let mut rules = Vec::new();

        // Migrate exclusions to route to direct
        if let Some(exclude) = json.get("exclude") {
            if let Some(domains) = exclude.get("domains") {
                rules.push(serde_json::json!({
                    "domain_suffix": domains,
                    "outbound": "direct"
                }));
            }
            if let Some(ips) = exclude.get("ips") {
                rules.push(serde_json::json!({
                    "ip_cidr": ips,
                    "outbound": "direct"
                }));
            }
            if let Some(processes) = exclude.get("processes") {
                rules.push(serde_json::json!({
                    "process_name": processes,
                    "outbound": "direct"
                }));
            }
        }

        new_json["routing"] = serde_json::json!({
            "rules": rules,
            "default_outbound": "proxy"
        });

        // 5. Preserve GUI state
        if let Some(gui) = json.get("gui") {
            new_json["gui"] = gui.clone();
        }
        
        (new_json, true)
    }
}
