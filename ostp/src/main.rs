use anyhow::{anyhow, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "OSTP Core - Ospab Stealth Transport Protocol", long_about = None)]
struct Args {
    /// Path to the JSON configuration file
    #[arg(long, default_value = "config.json")]
    config: PathBuf,

    /// Optional mode to initialize the config for (client or server)
    #[arg(short, long)]
    init: Option<String>,

    /// Generate a new secure access key and exit
    #[arg(short = 'g', long)]
    generate_key: bool,

    /// Format for generated key (hex, base64)
    #[arg(long, default_value = "hex")]
    format: String,

    /// Number of keys to generate
    #[arg(short = 'c', long, default_value_t = 1)]
    count: usize,

    /// Output ready-to-use client sharing links (ostp://...) from the server configuration
    #[arg(long)]
    links: bool,

    /// Optional client connection share link (ostp://ACCESS_KEY@HOST:PORT) to run instantly
    url: Option<String>,
}

fn parse_ostp_link(link: &str) -> Result<ClientConfig> {
    let parsed = url::Url::parse(link)
        .map_err(|e| anyhow!("Failed to parse share link URL: {e}"))?;

    if parsed.scheme() != "ostp" {
        anyhow::bail!("Unsupported URL scheme '{}', expected 'ostp://'", parsed.scheme());
    }

    let access_key = parsed.username().to_string();
    if access_key.is_empty() {
        anyhow::bail!("Missing access key (userinfo segment) in share link");
    }

    let host = parsed.host_str().ok_or_else(|| anyhow!("Missing host in share link"))?;
    let port = parsed.port().ok_or_else(|| anyhow!("Missing port in share link"))?;
    let server = format!("{host}:{port}");

    Ok(ClientConfig {
        server,
        access_key,
        socks5_bind: Some("127.0.0.1:1088".to_string()), // Fallback to standard SOCKS5 port
        tun: Some(TunConfig {
            enable: false, // Default to proxy, configurable via settings GUI
            wintun_path: Some("./wintun.dll".to_string()),
            ipv4_address: Some("10.1.0.2/24".to_string()),
            dns: None,
        }),
        turn: None,
        debug: Some(false),
        exclude: None,
        mux: None,
    })
}

fn generate_secure_key(format_type: &str) -> String {
    use rand::RngCore;
    let mut key = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut key);
    match format_type.to_lowercase().as_str() {
        "base64" => {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(key)
        }
        _ => key.iter().map(|b| format!("{:02x}", b)).collect(),
    }
}

fn parse_outbound_action(value: Option<String>) -> ostp_server::OutboundAction {
    match value.as_deref() {
        Some("direct") => ostp_server::OutboundAction::Direct,
        _ => ostp_server::OutboundAction::Proxy,
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
enum AppMode {
    Server(ServerConfig),
    Client(ClientConfig),
}

#[derive(Debug, Deserialize, Serialize)]
struct UnifiedConfig {
    #[serde(flatten)]
    mode: AppMode,
    log_level: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ServerConfig {
    listen: String,
    access_keys: Vec<String>,
    turn_server: Option<String>,
    debug: Option<bool>,
    outbound: Option<OutboundConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ClientConfig {
    server: String,
    access_key: String,
    socks5_bind: Option<String>,
    tun: Option<TunConfig>,
    turn: Option<TurnConfigRaw>,
    debug: Option<bool>,
    exclude: Option<ExcludeConfig>,
    mux: Option<MuxConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct TunConfig {
    enable: bool,
    wintun_path: Option<String>,
    ipv4_address: Option<String>,
    dns: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TurnConfigRaw {
    enabled: bool,
    server_addr: String,
    username: Option<String>,
    access_key: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OutboundConfig {
    enabled: bool,
    protocol: String,
    address: String,
    port: u16,
    #[serde(default)]
    rules: Vec<OutboundRule>,
    default_action: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OutboundRule {
    domain_suffix: Option<Vec<String>>,
    ip_cidr: Option<Vec<String>>,
    action: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ExcludeConfig {
    domains: Option<Vec<String>>,
    ips: Option<Vec<String>>,
    processes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct MuxConfig {
    enabled: Option<bool>,
    sessions: Option<usize>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let res = run_app().await;
    if let Err(e) = res {
        eprintln!("\n====================================================");
        eprintln!("[FATAL ERROR] Program terminated unexpectedly:");
        eprintln!("  {}", e);
        eprintln!("====================================================");
        
        #[cfg(target_os = "windows")]
        {
            println!("\nPress ENTER key to close this window...");
            let mut dummy = String::new();
            let _ = std::io::stdin().read_line(&mut dummy);
        }
        std::process::exit(1);
    }
    Ok(())
}

async fn run_app() -> Result<()> {
    let args = Args::parse();
    
    if args.generate_key {
        for _ in 0..args.count {
            println!("{}", generate_secure_key(&args.format));
        }
        return Ok(());
    }

    if let Some(url) = args.url {
        println!("[OSTP Core] Booting direct client connection via share link...");
        let client_cfg = parse_ostp_link(&url)
            .map_err(|e| anyhow!("Share Link Error: {e}"))?;
        return run_client_directly(client_cfg).await;
    }

    // Handle explicit configuration initialization
    if let Some(ref mode_str) = args.init {
        let is_server = mode_str == "server";
        let dummy = if is_server {
            UnifiedConfig {
                log_level: Some("info".to_string()),
                mode: AppMode::Server(ServerConfig {
                    listen: "0.0.0.0:50000".to_string(),
                    access_keys: vec![generate_secure_key("hex")],
                    turn_server: None,
                    debug: Some(false),
                    outbound: Some(OutboundConfig {
                        enabled: false,
                        protocol: "".to_string(),
                        address: "".to_string(),
                        port: 0,
                        rules: Vec::new(),
                        default_action: Some("direct".to_string()),
                    }),
                }),
            }
        } else {
            UnifiedConfig {
                log_level: Some("info".to_string()),
                mode: AppMode::Client(ClientConfig {
                    server: "127.0.0.1:50000".to_string(),
                    access_key: generate_secure_key("hex"),
                    socks5_bind: Some("127.0.0.1:1088".to_string()),
                    tun: Some(TunConfig {
                        enable: false,
                        wintun_path: Some("./wintun.dll".to_string()),
                        ipv4_address: Some("10.1.0.2/24".to_string()),
                        dns: None,
                    }),
                    turn: None,
                    debug: Some(false),
                    exclude: Some(ExcludeConfig {
                        domains: Some(Vec::new()),
                        ips: Some(Vec::new()),
                        processes: Some(Vec::new()),
                    }),
                    mux: Some(MuxConfig {
                        enabled: Some(false),
                        sessions: Some(1),
                    }),
                }),
            }
        };
        fs::write(&args.config, serde_json::to_string_pretty(&dummy)?)?;
        println!("Successfully initialized configuration at {:?}", args.config);
        
        if is_server {
            if let AppMode::Server(s) = dummy.mode {
                let key = &s.access_keys[0];
                println!("\n>>> Handy Client Share Link for your users:");
                println!("  ostp://{}@<YOUR_SERVER_PUBLIC_IP>:50000", key);
            }
        }
        return Ok(());
    }

    // Validate config file existence
    if !args.config.exists() {
        anyhow::bail!(
            "Configuration file {:?} not found.\n\n\
             To generate a default configuration template, run:\n\
             \t./ostp --init server\n\
             \tor\n\
             \t./ostp --init client\n\n\
             Or specify a custom configuration file path using:\n\
             \t./ostp --config /path/to/your_config.json",
            args.config
        );
    }

    let config_content = fs::read_to_string(&args.config)?;
    let config: UnifiedConfig = serde_json::from_str(&config_content)
        .map_err(|e| anyhow!("Failed to parse config: {}", e))?;

    if args.links {
        match config.mode {
            AppMode::Server(server_cfg) => {
                let listen = server_cfg.listen.clone();
                let parts: Vec<&str> = listen.split(':').collect();
                let port = parts.get(1).unwrap_or(&"50000");
                let host = if parts[0] == "0.0.0.0" { "<YOUR_SERVER_PUBLIC_IP>" } else { parts[0] };
                
                println!("\n>>> Ready-to-use OSTP client share links from {:?}:", args.config);
                for (idx, key) in server_cfg.access_keys.iter().enumerate() {
                    println!("  [{}] ostp://{}@{}:{}", idx + 1, key, host, port);
                }
                return Ok(());
            }
            AppMode::Client(_) => {
                anyhow::bail!("The configuration file is in Client mode. The --links flag can only extract keys from a Server configuration.");
            }
        }
    }

    match config.mode {
        AppMode::Server(server_cfg) => {
            println!("[OSTP Core] Starting in SERVER mode on {}", server_cfg.listen);
            if let Some(turn) = server_cfg.turn_server {
                println!("[OSTP Core] TURN integration enabled: {}", turn);
            }
            // Temporarily pass control to the isolated server implementation
            let debug = server_cfg.debug.unwrap_or(false);
            let outbound = server_cfg.outbound.map(|o| ostp_server::OutboundConfig {
                enabled: o.enabled,
                protocol: o.protocol,
                address: o.address,
                port: o.port,
                rules: o
                    .rules
                    .into_iter()
                    .map(|r| ostp_server::OutboundRule {
                        domain_suffix: r.domain_suffix.unwrap_or_default(),
                        ip_cidr: r.ip_cidr.unwrap_or_default(),
                        action: parse_outbound_action(r.action),
                    })
                    .collect(),
                default_action: parse_outbound_action(o.default_action),
            });
            ostp_server::run_server(server_cfg.listen, server_cfg.access_keys, outbound, debug).await?;
        }
        AppMode::Client(client_cfg) => {
            run_client_directly(client_cfg).await?;
        }

    }

    Ok(())
}

async fn run_client_directly(client_cfg: ClientConfig) -> Result<()> {
    println!("[OSTP Core] Starting in CLIENT mode connecting to {}", client_cfg.server);
    if let Some(ref tun) = client_cfg.tun {
        if tun.enable {
            println!("[OSTP Core] TUN mode enabled.");
            if let Some(ref path) = tun.wintun_path {
                println!("[OSTP Core] Using custom wintun path: {}", path);
            }
        }
    }
    println!("[OSTP Core] Client logic loaded.");
    let is_tun_enabled = client_cfg.tun.as_ref().map(|t| t.enable).unwrap_or(false);
    let turn_cfg = client_cfg.turn.as_ref();
    let client_conf = ostp_client::config::ClientConfig {
        mode: if is_tun_enabled { "tun".to_string() } else { "proxy".to_string() },
        debug: client_cfg.debug.unwrap_or(false),
        ostp: ostp_client::config::OstpConfig {
            server_addr: client_cfg.server.clone(),
            local_bind_addr: "0.0.0.0:0".to_string(),
            access_key: client_cfg.access_key.clone(),
            handshake_timeout_ms: 5000,
            io_timeout_ms: 5000,
        },
        local_proxy: ostp_client::config::LocalProxyConfig {
            bind_addr: client_cfg.socks5_bind.clone().unwrap_or_else(|| "127.0.0.1:1088".to_string()),
            connect_timeout_ms: 5000,
        },
        turn: ostp_client::config::TurnConfig {
            enabled: turn_cfg.map(|t| t.enabled).unwrap_or(false),
            server_addr: turn_cfg.and_then(|t| Some(t.server_addr.clone())).unwrap_or_default(),
            username: turn_cfg.and_then(|t| t.username.clone()).unwrap_or_default(),
            access_key: turn_cfg.and_then(|t| t.access_key.clone()).unwrap_or_default(),
        },
        exclusions: ostp_client::config::ExclusionConfig {
            domains: client_cfg.exclude.as_ref().and_then(|e| e.domains.clone()).unwrap_or_default(),
            ips: client_cfg.exclude.as_ref().and_then(|e| e.ips.clone()).unwrap_or_default(),
            processes: client_cfg.exclude.as_ref().and_then(|e| e.processes.clone()).unwrap_or_default(),
        },
        multiplex: ostp_client::config::MultiplexConfig {
            enabled: client_cfg.mux.as_ref().and_then(|m| m.enabled).unwrap_or(false),
            sessions: client_cfg.mux.as_ref().and_then(|m| m.sessions).unwrap_or(1),
        },
        dns_server: client_cfg.tun.as_ref().and_then(|t| t.dns.clone()),
    };
    // Run the client implementation
    ostp_client::runner::run_client(client_conf).await?;
    Ok(())
}
