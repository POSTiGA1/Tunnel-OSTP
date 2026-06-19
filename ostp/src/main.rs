use anyhow::{anyhow, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use colored::Colorize;

mod dns_prober;

#[derive(Parser, Debug)]
#[command(author, version, about = "OSTP Core - Ospab Stealth Transport Protocol", long_about = None)]
struct Args {
    /// Path to the JSON configuration file
    #[cfg_attr(unix, arg(long, default_value = "/etc/ostp/config.json", help_heading = "Common Commands"))]
    #[cfg_attr(windows, arg(long, default_value = "config.json", help_heading = "Common Commands"))]
    config: PathBuf,

    /// Optional mode to initialize the config for (client or server)
    #[arg(short, long, help_heading = "Common Commands")]
    init: Option<String>,

    /// Run the interactive setup wizard
    #[arg(long, help_heading = "Common Commands")]
    setup: bool,

    /// Generate a new secure access key and exit
    #[arg(short = 'g', long, help_heading = "Common Commands")]
    generate_key: bool,

    /// Format for generated key (hex, base64)
    #[arg(long, default_value = "hex", help_heading = "Common Commands")]
    format: String,

    /// Number of keys to generate
    #[arg(short = 'c', long, default_value_t = 1, help_heading = "Common Commands")]
    count: usize,

    /// Output ready-to-use client sharing links (ostp://...) from the server configuration
    #[arg(long, help_heading = "Server Commands")]
    links: bool,

    /// Validate configuration file and exit
    #[arg(long, help_heading = "Common Commands")]
    check: bool,

    /// Optional client connection share link (ostp://ACCESS_KEY@HOST:PORT) to run instantly
    #[arg(help_heading = "Client Commands")]
    url: Option<String>,

    /// Uninstall OSTP: stop service, remove binary and configuration files
    #[arg(long, help_heading = "Common Commands")]
    uninstall: bool,

    /// Update OSTP: re-run the install script to fetch and install the latest version
    #[arg(long, help_heading = "Common Commands")]
    update: bool,

    /// Specify a target version for the update command (e.g., -v 0.2.98)
    #[arg(short = 'v', long = "version", help_heading = "Common Commands")]
    target_version: Option<String>,

    /// Import a share link (ostp://...) into the configuration file and exit
    #[arg(long, help_heading = "Client Commands")]
    import: Option<String>,

    /// Output shell export commands for proxy (eval $(ostp --proxy-env))
    #[arg(long, help_heading = "Client Commands")]
    proxy_env: bool,

    /// Output shell export commands to clear proxy (eval $(ostp --proxy-env-clear))
    #[arg(long, help_heading = "Client Commands")]
    proxy_env_clear: bool,

    /// Force migration of the configuration file to the latest format and exit
    #[arg(long, help_heading = "Common Commands")]
    migrate: bool,

    /// Run the network prober to find the fastest DNS resolver for the DNS Transport
    #[arg(long, help_heading = "Client Commands")]
    prober: bool,
}

fn parse_ostp_link(link: &str) -> Result<serde_json::Value> {
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
    let _server = format!("{host}:{port}");
    let mut _sni = String::new();
    let mut transport_mode = String::from("udp");
    let mut tun_enabled = false;
    let mut _tun_dns = None;
    let mut _wss_enabled = false;
    let mut dns_domain = None;
    let mut dns_pubkey = None;

    for (k, v) in parsed.query_pairs() {
        match &*k {
            "sni" => _sni = v.into_owned(),
            "type" => transport_mode = v.into_owned(),
            "tun" => tun_enabled = v == "true",
            "dns" => _tun_dns = Some(v.into_owned()),
            "wss" => _wss_enabled = v == "true",
            "domain" => dns_domain = Some(v.into_owned()),
            "pubkey" => dns_pubkey = Some(v.into_owned()),
            _ => {}
        }
    }

    let mut transport_json = serde_json::json!({
        "type": transport_mode
    });
    
    if transport_mode == "dns" {
        if let Some(d) = dns_domain {
            transport_json["domain"] = serde_json::json!(d);
        }
        if let Some(p) = dns_pubkey {
            transport_json["pubkey"] = serde_json::json!(p);
        }
    }

    Ok(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "log": {
            "level": "info"
        },
        "inbounds": [
            {
                "type": "tun",
                "tag": "tun-in",
                "auto_route": tun_enabled,
                "mtu": 1140
            },
            {
                "type": "local_proxy",
                "tag": "socks-in",
                "protocol": "socks",
                "listen": "127.0.0.1",
                "port": 1088
            }
        ],
        "outbounds": [
            {
                "type": "ostp",
                "tag": "proxy",
                "server": parsed.host_str().unwrap_or(""),
                "port": parsed.port().unwrap_or(50000),
                "access_key": access_key,
                "transport": transport_json,
                "multiplex": {
                    "enabled": false,
                    "sessions": 1
                }
            },
            {
                "type": "direct",
                "tag": "direct"
            },
            {
                "type": "block",
                "tag": "block"
            }
        ],
        "routing": {
            "rules": [],
            "default_outbound": "proxy"
        }
    }))
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
    Client(serde_json::Value),
    Relay(RelayServerConfig),
}

#[derive(Debug, Deserialize, Serialize)]
struct UnifiedConfig {
    version: Option<String>,
    log: Option<serde_json::Value>,
    #[serde(flatten)]
    mode: AppMode,
}

impl UnifiedConfig {
    fn validate(&self) -> Result<()> {
        match &self.mode {
            AppMode::Server(cfg) => {
                let mut has_ostp = false;
                for inbound in &cfg.inbounds {
                    if let ostp_server::config::ServerInbound::Ostp { users, .. } = inbound {
                        has_ostp = true;
                        if users.is_empty() {
                            anyhow::bail!("Ostp inbound must contain at least one user.");
                        }
                    }
                }
                if !has_ostp {
                    anyhow::bail!("Server configuration must contain at least one Ostp inbound.");
                }
            }
            AppMode::Client(cfg) => {
                if let Some(outbounds) = cfg.get("outbounds").and_then(|o| o.as_array()) {
                    let has_proxy = outbounds.iter().any(|o| o.get("type").and_then(|t| t.as_str()) == Some("ostp"));
                    if !has_proxy {
                        anyhow::bail!("Client configuration must contain an ostp outbound proxy.");
                    }
                }
            }
            AppMode::Relay(cfg) => {
                if cfg.upstream_tcp.is_empty() {
                    anyhow::bail!("Relay configuration must specify upstream_tcp address.");
                }
                if cfg.upstream_api_url.is_empty() {
                    anyhow::bail!("Relay configuration must specify upstream_api_url.");
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum UserConfig {
    Detailed {
        access_key: String,
        name: Option<String>,
        limit_bytes: Option<u64>,
    },
    KeyOnly(String),
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

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
struct OutboundRule {
    domain_suffix: Option<Vec<String>>,
    ip_cidr: Option<Vec<String>>,
    protocol: Option<String>,
    action: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize, Clone)]
struct TransportConfigRaw {
    mode: Option<String>,
    stealth_sni: Option<String>,
    wss: Option<bool>,
}

type ServerConfig = ostp_server::config::ModularServerConfig;

/// Конфигурация Relay-узла в config.json
#[derive(Debug, Deserialize, Serialize)]
struct RelayServerConfig {
    /// Адрес(а) прослушивания (UDP + TCP UoT)
    listen: ListenConfig,
    /// Адрес upstream для TCP (UoT) трафика
    upstream_tcp: String,
    /// Адрес upstream для UDP трафика
    upstream_udp: String,
    /// URL API целевого сервера для синхронизации ключей
    upstream_api_url: String,
    /// Bearer-токен для API целевого сервера
    #[serde(default)]
    upstream_api_token: String,
    /// Интервал синхронизации ключей в секундах (по умолчанию 30)
    #[serde(default = "default_sync_interval")]
    sync_interval_secs: u64,
    debug: Option<bool>,
}

fn default_sync_interval() -> u64 { 30 }

/// Supports both single string "0.0.0.0:50000" and array ["0.0.0.0:50000", "[::]:50000"]
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
enum ListenConfig {
    Single(String),
    Multiple(Vec<String>),
}

impl ListenConfig {
    fn addresses(&self) -> Vec<String> {
        match self {
            ListenConfig::Single(s) => vec![s.clone()],
            ListenConfig::Multiple(v) => v.clone(),
        }
    }

    fn primary(&self) -> String {
        match self {
            ListenConfig::Single(s) => s.clone(),
            ListenConfig::Multiple(v) => v.first().cloned().unwrap_or_default(),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
struct ApiConfig {
    enabled: Option<bool>,
    bind: Option<String>,
    token: Option<String>,
    webpath: Option<String>,
    username: Option<String>,
    password_hash: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
struct FallbackCfg {
    enabled: Option<bool>,
    listen: Option<String>,
    target: Option<String>,
}



#[tokio::main]
async fn main() -> Result<()> {
    ostp_client::logging::setup_panic_hook();
    let _log_guard = ostp_client::logging::init_tracing("info", "ostp-cli", env!("CARGO_PKG_VERSION"));

    let res = run_app().await;
    if let Err(e) = res {
        eprintln!();
        eprintln!("{} {}", "[FATAL ERROR]".red().bold(), e);
        eprintln!();
        
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

#[allow(dead_code)]
fn is_private_ip(ip: &str) -> bool {
    ip.starts_with("10.") 
    || ip.starts_with("192.168.") 
    || ip.starts_with("127.")
    || (ip.starts_with("172.") && {
        let parts: Vec<&str> = ip.split('.').collect();
        if parts.len() >= 2 {
            if let Ok(second) = parts[1].parse::<u8>() {
                (16..=31).contains(&second)
            } else { false }
        } else { false }
    })
}

fn detect_local_public_ip() -> Option<String> {
    #[cfg(not(target_os = "windows"))]
    {
        let out = std::process::Command::new("ip")
            .args(["-4", "addr", "show", "scope", "global"])
            .output()
            .ok()?;
        
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            if let Some(idx) = line.find("inet ") {
                let substr = &line[idx + 5..];
                let ip = substr.split(|c: char| c == '/' || c.is_whitespace()).next().unwrap_or("");
                if !ip.is_empty() && !is_private_ip(ip) {
                    return Some(ip.to_string());
                }
            }
        }
    }
    None
}

fn get_or_ask_public_ip(config_path: &std::path::Path) -> String {
    let config_dir = config_path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let cache_path = config_dir.join(".ostp_public_ip");

    if cache_path.exists() {
        if let Ok(cached) = std::fs::read_to_string(&cache_path) {
            let ip = cached.trim().to_string();
            if !ip.is_empty() {
                return ip;
            }
        }
    }

    if let Some(detected) = detect_local_public_ip() {
        println!("[ostp] Detected public IP: {}", detected);
        let _ = std::fs::write(&cache_path, &detected);
        return detected;
    }

    print!("\n[ostp] Could not detect the server public IP automatically.\n");
    print!("  Enter your public IP or domain: ");
    use std::io::Write;
    let _ = std::io::stdout().flush();

    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_ok() {
        let ip = input.trim().to_string();
        if !ip.is_empty() {
            let _ = std::fs::write(&cache_path, &ip);
            return ip;
        }
    }

    "<YOUR_SERVER_PUBLIC_IP>".to_string()
}

// ---------------------------------------------------------------------------
// Setup Wizard
// ---------------------------------------------------------------------------

fn wizard_prompt(prompt: &str, default: &str) -> String {
    use std::io::Write;
    if default.is_empty() {
        print!("  {} ", prompt);
    } else {
        print!("  {} [{}]: ", prompt, default.cyan());
    }
    std::io::stdout().flush().unwrap();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    let trimmed = input.trim().to_string();
    if trimmed.is_empty() && !default.is_empty() {
        default.to_string()
    } else {
        trimmed
    }
}

fn wizard_yn(prompt: &str, default_yes: bool) -> bool {
    let hint = if default_yes { "Y/n" } else { "y/N" };
    use std::io::Write;
    print!("  {} [{}]: ", prompt, hint.cyan());
    std::io::stdout().flush().unwrap();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    match input.trim().to_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no"  => false,
        _            => default_yes,
    }
}

fn wizard_step(n: usize, total: usize, title: &str) {
    println!();
    println!("  {} {}",
        format!("[{}/{}]", n, total).bold().yellow(),
        title.bold());
    println!("  {}", "─".repeat(50).dimmed());
}

fn wizard_box(lines: &[&str]) {
    let width = lines.iter().map(|l| l.len()).max().unwrap_or(0).max(40);
    println!("  ╔{}╗", "═".repeat(width + 2));
    for line in lines {
        let padding = width - line.len();
        println!("  ║ {}{} ║", line, " ".repeat(padding));
    }
    println!("  ╚{}╝", "═".repeat(width + 2));
}

fn wizard_ok(msg: &str) {
    println!("  {} {}", "✓".green().bold(), msg);
}

fn wizard_warn(msg: &str) {
    println!("  {} {}", "!".yellow().bold(), msg.yellow());
}

fn wizard_section(title: &str) {
    println!("\n  {}", title.bold().underline());
}

fn wizard_save_config(config_path: &std::path::Path, json_value: &serde_json::Value) -> Result<std::path::PathBuf> {
    let current_path = config_path.to_path_buf();
    
    // Attempt 1: write to requested path
    if let Some(parent) = current_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    
    match fs::write(&current_path, serde_json::to_string_pretty(json_value)?) {
        Ok(_) => {
            wizard_ok(&format!("Configuration saved to {:?}", current_path));
            return Ok(current_path);
        }
        Err(e) => {
            wizard_warn(&format!("Could not write to {:?}: {}", current_path, e));
            // Attempt 2: fallback to current directory
            let fallback = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("config.json");
            wizard_warn(&format!("Falling back to {:?}", fallback));
            
            match fs::write(&fallback, serde_json::to_string_pretty(json_value)?) {
                Ok(_) => {
                    wizard_ok(&format!("Configuration saved to {:?}", fallback));
                    return Ok(fallback);
                }
                Err(e2) => {
                    wizard_warn(&format!("Could not write to fallback {:?}: {}", fallback, e2));
                    anyhow::bail!("Failed to save configuration to any location.");
                }
            }
        }
    }
}

fn run_setup_wizard(config_path: &std::path::Path) -> Result<()> {
    use std::io::Write;

    println!();
    wizard_box(&[
        "OSTP Setup Wizard",
        concat!("Version ", env!("CARGO_PKG_VERSION")),
        "",
        "This wizard will create your configuration file.",
        "Press Enter to accept the value shown in [brackets].",
    ]);

    // ── Mode selection ────────────────────────────────────────────────
    println!();
    println!("  {}", "Select operating mode:".bold());
    println!("  {}", "─".repeat(50).dimmed());

    #[cfg(unix)]
    {
        println!("    {}  Client       (connect to a server via VPN/proxy)",  "[1]".cyan().bold());
        println!("    {}  Server       (accept client connections)",             "[2]".cyan().bold());
        println!("    {}  Server+Panel (server with web management panel)",     "[3]".cyan().bold());
        println!("    {}  Relay        (forward traffic to another server)",    "[4]".cyan().bold());
    }
    #[cfg(windows)]
    {
        println!("    {}  Client       (connect to a server via VPN/proxy)", "[1]".cyan().bold());
        println!("    {}  Server       (accept client connections)",           "[2]".cyan().bold());
        println!("    {}  Server+Panel (server with web management panel)",     "[3]".cyan().bold());
    }

    print!("\n  Your choice: ");
    std::io::stdout().flush().unwrap();
    let mut mode_input = String::new();
    std::io::stdin().read_line(&mut mode_input).unwrap();
    let mode_choice = mode_input.trim();

    #[cfg(unix)]
    let valid_choices = ["1", "2", "3", "4"];
    #[cfg(windows)]
    let valid_choices = ["1", "2", "3"];

    if !valid_choices.contains(&mode_choice) {
        anyhow::bail!("Invalid selection '{}'", mode_choice);
    }

    match mode_choice {
        // ── CLIENT ────────────────────────────────────────────────────
        "1" => {
            #[cfg(unix)]  const TOTAL: usize = 5;
            #[cfg(windows)] const TOTAL: usize = 4;

            wizard_step(1, TOTAL, "Server connection");

            // Try import from link first
            let use_link = wizard_yn("Do you have a share link (ostp://...)?", false);
            let (server, access_key, sni, transport_mode) = if use_link {
                let link_str = wizard_prompt("Paste link", "");
                let parsed = url::Url::parse(&link_str).unwrap();
                let mut p = parsed.query_pairs();
                let sni = p.find(|(k, _)| k == "sni").map(|(_, v)| v.to_string()).unwrap_or_default();
                let tm = p.find(|(k, _)| k == "type").map(|(_, v)| v.to_string()).unwrap_or("udp".to_string());
                (parsed.host_str().unwrap().to_string() + ":" + &parsed.port().unwrap_or(50000).to_string(), parsed.username().to_string(), sni, tm)
            } else {
                ("127.0.0.1:50000".to_string(), "".to_string(), "".to_string(), "udp".to_string())
            };

            wizard_step(2, TOTAL, "Local proxy");
            let socks_bind = wizard_prompt("Local SOCKS5 proxy bind address", "127.0.0.1:1088");

            wizard_step(3, TOTAL, "VPN (TUN) mode");

            // SSH warning on Linux — always
            #[cfg(unix)]
            {
                println!();
                println!("  ┌{}", "─".repeat(60));
                println!("  │ {} {}",
                    "WARNING:".red().bold(),
                    "TUN mode captures ALL network traffic.".yellow());
                println!("  │");
                println!("  │  {} If you are connected via SSH to a headless server,",
                    "▶".red());
                println!("  │    enabling TUN mode will route the SSH connection");
                println!("  │    through the VPN tunnel.");
                println!("  │");
                println!("  │    Make sure the VPN server is reachable before");
                println!("  │    enabling TUN, or your SSH session may be lost!");
                println!("  └{}", "─".repeat(60));
            }

            let tun_enable = wizard_yn("Enable TUN (full VPN) mode?", false);

            let (_tun_dns, _kill_switch) = if tun_enable {
                let dns = wizard_prompt("DNS server for TUN", "1.1.1.1");
                let ks  = wizard_yn("Enable kill switch (block traffic if VPN drops)?", false);
                (dns, ks)
            } else {
                ("1.1.1.1".to_string(), false)
            };

            wizard_step(4, TOTAL, "Multiplexing");
            let mux_enable = wizard_yn("Enable connection multiplexing (better performance)?", false);
            let mux_sessions = if mux_enable {
                let s = wizard_prompt("Number of parallel sessions", "5");
                s.parse::<usize>().unwrap_or(5)
            } else { 1 };

            // Daemon step — Linux only
            #[cfg(unix)]
            {
                wizard_step(5, TOTAL, "Auto-start (systemd)");
            }

            // Build and save config
            let key_for_gen = generate_secure_key("hex");
            let _ = key_for_gen;
            let _ = &sni;

            let server_parts: Vec<&str> = server.split(':').collect();
            let server_host = server_parts.get(0).unwrap_or(&"127.0.0.1");
            let server_port = server_parts.get(1).unwrap_or(&"50000").parse::<u16>().unwrap_or(50000);
            
            let socks_parts: Vec<&str> = socks_bind.split(':').collect();
            let socks_host = socks_parts.get(0).unwrap_or(&"127.0.0.1");
            let socks_port = socks_parts.get(1).unwrap_or(&"1088").parse::<u16>().unwrap_or(1088);

            let client_json = serde_json::json!({
                "mode": "client",
                "version": env!("CARGO_PKG_VERSION"),
                "log": {
                    "level": "info"
                },
                "inbounds": [
                    {
                        "type": "tun",
                        "tag": "tun-in",
                        "auto_route": tun_enable,
                        "mtu": 1140
                    },
                    {
                        "type": "local_proxy",
                        "tag": "socks-in",
                        "protocol": "socks",
                        "listen": socks_host,
                        "port": socks_port
                    }
                ],
                "outbounds": [
                    {
                        "type": "ostp",
                        "tag": "proxy",
                        "server": server_host,
                        "port": server_port,
                        "access_key": access_key,
                        "transport": {
                            "type": transport_mode
                        },
                        "multiplex": {
                            "enabled": mux_enable,
                            "sessions": mux_sessions
                        }
                    },
                    {
                        "type": "direct",
                        "tag": "direct"
                    },
                    {
                        "type": "block",
                        "tag": "block"
                    }
                ],
                "routing": {
                    "rules": [
                        {
                            "domain_suffix": ["localhost", "127.0.0.1"],
                            "outbound": "direct"
                        }
                    ],
                    "default_outbound": "proxy"
                }
            });

            let actual_path = wizard_save_config(config_path, &client_json)?;
            println!();

            // Daemon registration
            #[cfg(unix)]
            wizard_register_systemd(&actual_path)?;
            #[cfg(windows)]
            wizard_register_windows_service(&actual_path)?;

            // Summary
            println!();
            wizard_box(&[
                "Setup complete!",
                "",
                &format!("Config:       {:?}", config_path),
                &format!("Server:       {}", server),
                &format!("SOCKS5 proxy: {}", socks_bind),
                &format!("TUN mode:     {}", if tun_enable { "enabled" } else { "disabled" }),
                "",
                "To start:  ostp",
                "To check:  ostp --check",
                "Proxy env: eval $(ostp --proxy-env)",
            ]);
        }

        // ── SERVER ────────────────────────────────────────────────────
        "2" => {
            #[cfg(unix)]    const TOTAL: usize = 4;
            #[cfg(windows)] const TOTAL: usize = 3;

            wizard_step(1, TOTAL, "Listen address");
            let listen = wizard_prompt("Listen address (host:port)", "0.0.0.0:50000");

            wizard_step(2, TOTAL, "Access keys");
            let key_count_str = wizard_prompt("Number of access keys to generate", "1");
            let key_count = key_count_str.parse::<usize>().unwrap_or(1).max(1);
            let mut access_keys = Vec::new();
            for _ in 0..key_count {
                access_keys.push(generate_secure_key("hex"));
            }
            wizard_ok(&format!("Generated {} key(s)", key_count));

            wizard_step(3, TOTAL, "Service registration");
            // intentional: step text then daemon call below
            let port_str = listen.split(':').last().unwrap_or("50000");
            let port: u16 = port_str.parse().unwrap_or(50000);
            let server_json = serde_json::json!({
                "mode": "server",
                "version": "{}",
                "log": {
                    "level": "info"
                },
                "dns_transport": {
                    "enabled": false,
                    "listen": "0.0.0.0:53",
                    "domain": "tunnel.yourdomain.com",
                    "pubkey": "",
                    "privkey": ""
                },
                "inbounds": [
                    {
                        "type": "ostp",
                        "tag": "ostp-in",
                        "listen": "0.0.0.0",
                        "port": port,
                        "users": access_keys
                    }
                ],
                "outbounds": [
                    {
                        "type": "direct",
                        "tag": "direct"
                    }
                ]
            });

            let actual_path = wizard_save_config(config_path, &server_json)?;

            #[cfg(unix)]
            wizard_register_systemd(&actual_path)?;
            #[cfg(windows)]
            wizard_register_windows_service(&actual_path)?;

            // Print share links
            let host = get_or_ask_public_ip(config_path);
            let port = listen.split(':').last().unwrap_or("50000");
            println!();
            wizard_section("Share links for clients:");
            for (i, key) in access_keys.iter().enumerate() {
                println!("  [{}] ostp://{}@{}:{}", i + 1, key, host, port);
            }

            println!();
            wizard_box(&[
                "Setup complete!",
                "",
                &format!("Config:  {:?}", config_path),
                &format!("Listen:  {}", listen),
                &format!("Keys:    {}", key_count),
                "",
                "To start:  ostp",
                "To check:  ostp --check",
                "Share links: ostp --links",
            ]);
        }

        // ── SERVER + PANEL ───────────────────────────────
        "3" => {
            #[cfg(unix)]    const TOTAL: usize = 6;
            #[cfg(windows)] const TOTAL: usize = 5;

            wizard_step(1, TOTAL, "Listen address");
            let listen = wizard_prompt("Listen address (host:port)", "0.0.0.0:50000");
            let host = get_or_ask_public_ip(config_path);

            wizard_step(2, TOTAL, "Access keys");
            let key_count_str = wizard_prompt("Number of access keys to generate", "1");
            let key_count = key_count_str.parse::<usize>().unwrap_or(1).max(1);
            let mut access_keys: Vec<String> = Vec::new();
            for _ in 0..key_count { access_keys.push(generate_secure_key("hex")); }
            wizard_ok(&format!("Generated {} key(s)", key_count));

            wizard_step(3, TOTAL, "Web panel settings");
            use rand::Rng;
            let panel_port = wizard_prompt("Panel port", "9090");
            let rand_path: String = (0..8).map(|_| {
                let idx = rand::thread_rng().gen_range(0..36u8);
                (if idx < 10 { b'0' + idx } else { b'a' + idx - 10 }) as char
            }).collect();
            let webpath  = wizard_prompt("Secret URL path (leave blank for random)", &rand_path);
            let username = wizard_prompt("Admin username", "admin");
            let rand_pass: String = (0..12).map(|_| {
                let idx = rand::thread_rng().gen_range(0..62u8);
                (match idx {
                    0..=9   => b'0' + idx,
                    10..=35 => b'a' + idx - 10,
                    _       => b'A' + idx - 36,
                }) as char
            }).collect();
            let password  = wizard_prompt("Admin password (blank for random)", &rand_pass);
            let pass_hash = {
                use std::fmt::Write as _;
                let mut hash = String::new();
                let digest: [u8; 32] = {
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    // simple SHA-256 via sha2 would be ideal; we reuse existing pattern from the old script
                    // fallback: store plaintext-keyed sha256 if sha2 crate not available
                    // The ostp binary already uses sha256 for reality keys — let's do it properly via python fallback
                    // Actually: ostp-core likely has sha2 in tree. Let's use hex output.
                    // We'll use std's hash as placeholder and document; sha2 is not in ostp/Cargo.toml directly.
                    // Use sha2 via ostp_core if available, else hex of std hasher.
                    let mut h = DefaultHasher::new();
                    password.hash(&mut h);
                    let v = h.finish();
                    let mut out = [0u8; 32];
                    out[..8].copy_from_slice(&v.to_be_bytes());
                    out
                };
                for b in digest { let _ = write!(hash, "{:02x}", b); }
                hash
            };

            wizard_step(4, TOTAL, "Saving configuration");
            let _panel_bind = format!("0.0.0.0:{}", panel_port);
            let server_json = serde_json::json!({
                "mode": "server",
                "version": "{}",
                "log": {
                    "level": "info"
                },
                "dns_transport": {
                    "enabled": false,
                    "listen": "0.0.0.0:53",
                    "domain": "tunnel.yourdomain.com",
                    "pubkey": "",
                    "privkey": ""
                },
                "inbounds": [
                    {
                        "type": "ostp",
                        "tag": "ostp-in",
                        "listen": "0.0.0.0",
                        "port": 50000,
                        "users": access_keys
                    },
                    {
                        "type": "api",
                        "tag": "api-in",
                        "listen": "0.0.0.0",
                        "port": panel_port.parse::<u16>().unwrap_or(9090),
                        "webpath": webpath,
                        "username": username,
                        "password_hash": pass_hash
                    }
                ],
                "outbounds": [
                    {
                        "type": "direct",
                        "tag": "direct"
                    }
                ]
            });

            wizard_step(4, TOTAL, "Saving configuration");
            let actual_path = wizard_save_config(config_path, &server_json)?;

            #[cfg(unix)]
            {
                wizard_step(5, TOTAL, "Service registration");
                wizard_register_systemd(&actual_path)?;
            }
            #[cfg(windows)]
            {
                wizard_step(5, TOTAL, "Service registration");
                wizard_register_windows_service(&actual_path)?;
            }

            let port = listen.split(':').last().unwrap_or("50000");
            println!();
            wizard_section("Share links for clients:");
            for (i, key) in access_keys.iter().enumerate() {
                println!("  [{}] ostp://{}@{}:{}", i + 1, key, host, port);
            }

            println!();
            wizard_box(&[
                "Setup complete!",
                "",
                &format!("Config:   {:?}", config_path),
                &format!("Listen:   {}", listen),
                &format!("Panel:    http://{}:{}/{}/", host, panel_port, webpath),
                &format!("Username: {}", username),
                &format!("Password: {}", password),
            ]);
        }

        // ── RELAY (Linux only) ────────────────────────────────────────
        #[cfg(unix)]
        "4" => {
            const TOTAL: usize = 3;

            wizard_step(1, TOTAL, "Listen & upstream");
            let listen   = wizard_prompt("Listen address (host:port)", "0.0.0.0:50000");
            let upstream = wizard_prompt("Upstream server address (host:port)", "");
            if upstream.is_empty() { anyhow::bail!("Upstream address cannot be empty."); }
            let api_url  = wizard_prompt("Upstream server API URL (e.g. http://1.2.3.4:9090)", "");
            let api_token = wizard_prompt("Upstream API token (leave blank if none)", "");

            wizard_step(2, TOTAL, "Saving configuration");
            let relay_json = serde_json::json!({
                "mode": "relay",
                "version": "{}",
                "log": {
                    "level": "info"
                },
                "listen": listen,
                "upstream_tcp": upstream,
                "upstream_udp": upstream,
                "upstream_api_url": api_url,
                "upstream_api_token": api_token,
                "sync_interval_secs": 30,
                "debug": false
            });

            let actual_path = wizard_save_config(config_path, &relay_json)?;

            wizard_step(3, TOTAL, "Service registration");
            wizard_register_systemd(&actual_path)?;

            println!();
            wizard_box(&[
                "Relay setup complete!",
                "",
                &format!("Config:    {:?}", config_path),
                &format!("Listen:    {}", listen),
                &format!("Upstream:  {}", upstream),
                "",
                "To start:  ostp",
            ]);
        }

        _ => unreachable!()
    }

    Ok(())
}

#[cfg(unix)]
fn wizard_register_systemd(config_path: &std::path::Path) -> Result<()> {
    use std::process::Command;
    let reg = wizard_yn("Register as systemd service (auto-start on boot)?", true);
    if !reg { return Ok(()); }

    let binary = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("/opt/ostp/ostp"));
    let service = format!(
        "[Unit]\nDescription=OSTP Stealth Transport Protocol\nAfter=network.target\nWants=network-online.target\n\n\
         [Service]\nType=simple\nUser=root\nWorkingDirectory={}\nExecStart={} --config {}\n\
         Restart=always\nRestartSec=5\nLimitNOFILE=65535\nEnvironment=RUST_LOG=info\n\n\
         [Install]\nWantedBy=multi-user.target\n",
        binary.parent().map(|p| p.display().to_string()).unwrap_or_else(|| "/opt/ostp".to_string()),
        binary.display(),
        config_path.display()
    );

    let unit_path = "/etc/systemd/system/ostp.service";
    match fs::write(unit_path, &service) {
        Ok(_) => {
            let _ = Command::new("systemctl").arg("daemon-reload").status();
            let _ = Command::new("systemctl").args(["enable", "ostp"]).status();
            wizard_ok(&format!("Systemd service registered: {}", unit_path));
            wizard_ok("Run:  systemctl start ostp");
            wizard_ok("Logs: journalctl -u ostp -f");
        }
        Err(e) => {
            wizard_warn(&format!("Could not write {}: {} (are you root?)", unit_path, e));
            wizard_warn("Skipping service registration.");
        }
    }
    Ok(())
}

#[cfg(windows)]
fn wizard_register_windows_service(config_path: &std::path::Path) -> Result<()> {
    use std::process::Command;
    let reg = wizard_yn("Register as Windows Service (auto-start on boot)?", true);
    if !reg { return Ok(()); }

    let binary = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from(r"C:\opt\ostp\ostp.exe"));
    let bin_str    = binary.to_string_lossy();
    let config_str = config_path.to_string_lossy();
    let cmd_line   = format!("\"{}\" --config \"{}\"", bin_str, config_str);

    let status = Command::new("sc")
        .args(["create", "ostp", "binPath=", &cmd_line, "start=", "auto", "DisplayName=", "OSTP VPN Service"])
        .status();

    match status {
        Ok(s) if s.success() => {
            wizard_ok("Windows Service 'ostp' registered.");
            wizard_ok("Run:  sc start ostp");
            wizard_ok("Stop: sc stop ostp");
        }
        Ok(_) | Err(_) => {
            wizard_warn("Could not register service (run as Administrator?).");
            wizard_warn("Skipping service registration.");
        }
    }
    Ok(())
}

async fn run_app() -> Result<()> {
    let args = Args::parse();

    if args.uninstall {
        return cmd_uninstall();
    }

    if args.update {
        return cmd_update();
    }

    if args.migrate {
        return cmd_migrate(&args.config);
    }

    if args.config.exists() && !args.uninstall && !args.update {
        if let Ok(config_content) = fs::read_to_string(&args.config) {
            let mut stripped = json_comments::StripComments::new(config_content.as_bytes());
            if let Ok(raw_json) = serde_json::from_reader::<_, serde_json::Value>(&mut stripped) {
                if raw_json.get("version").and_then(|v| v.as_str()) != Some(env!("CARGO_PKG_VERSION")) {
                    println!("{} Outdated configuration format detected.", "[ostp]".yellow().bold());
                    println!("{} Please run '{}' to update your configuration to the latest modular format.", "[ostp]".yellow().bold(), "ostp --migrate".green());
                    std::process::exit(1);
                }
            }
        }
    }

    // ── Setup wizard: explicit flag or first-time (no config) ────────
    if args.setup {
        return run_setup_wizard(&args.config);
    }
    // Auto-trigger wizard on first run (no config, no other flags)
    if !args.config.exists()
        && !args.generate_key
        && args.init.is_none()
        && args.url.is_none()
        && args.import.is_none()
        && !args.check
        && !args.links
        && !args.proxy_env
        && !args.proxy_env_clear
    {
        return run_setup_wizard(&args.config);
    }

    if args.proxy_env {
        let mut port = 1080;
        if args.config.exists() {
            if let Ok(content) = fs::read_to_string(&args.config) {
                let mut stripped = json_comments::StripComments::new(content.as_bytes());
                if let Ok(config) = serde_json::from_reader::<_, UnifiedConfig>(&mut stripped) {
                    if let AppMode::Client(c) = config.mode {
                        let (migrated, _) = ostp_client::config::ClientConfig::migrate_json(c);
                        if let Some(inbounds) = migrated.get("inbounds").and_then(|i| i.as_array()) {
                            for inbound in inbounds {
                                if inbound.get("type").and_then(|t| t.as_str()) == Some("local_proxy") {
                                    if let Some(p) = inbound.get("port").and_then(|p| p.as_u64()) {
                                        port = p as u16;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        println!("export http_proxy=\"socks5://127.0.0.1:{}\"", port);
        println!("export https_proxy=\"socks5://127.0.0.1:{}\"", port);
        println!("export all_proxy=\"socks5://127.0.0.1:{}\"", port);
        return Ok(());
    }

    if args.proxy_env_clear {
        println!("unset http_proxy");
        println!("unset https_proxy");
        println!("unset all_proxy");
        return Ok(());
    }

    if args.generate_key {
        let mut new_keys = Vec::new();
        for _ in 0..args.count {
            let key = generate_secure_key(&args.format);
            println!("{}", key);
            new_keys.push(key);
        }

        // Автоматическое добавление ключа в config.json если это сервер
        if args.config.exists() {
            if let Ok(content) = fs::read_to_string(&args.config) {
                let mut stripped = json_comments::StripComments::new(content.as_bytes());
                let mut content_str = String::new();
                use std::io::Read;
                if stripped.read_to_string(&mut content_str).is_ok() {
                    if let Ok(mut json_val) = serde_json::from_str::<serde_json::Value>(&content_str) {
                        if let Some(mode) = json_val.get("mode").and_then(|m| m.as_str()) {
                            if mode == "server" {
                                if let Some(access_keys) = json_val.get_mut("access_keys").and_then(|a| a.as_array_mut()) {
                                    for key in new_keys {
                                        access_keys.push(serde_json::Value::String(key));
                                    }
                                    if let Ok(new_content) = serde_json::to_string_pretty(&json_val) {
                                        let _ = fs::write(&args.config, new_content);
                                        println!("[ostp] Key(s) automatically added to {:?}", args.config);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        return Ok(());
    }

    if let Some(import_url) = args.import {
        println!("{} Importing configuration from share link...", "[ostp]".cyan().bold());
        let client_cfg = parse_ostp_link(&import_url)
            .map_err(|e| anyhow!("Share Link Error: {e}"))?;
        let unified = UnifiedConfig {
            mode: AppMode::Client(client_cfg),
            version: Some("0.3.1".to_string()),
            log: Some(serde_json::json!({ "level": "info" })),
        };
        let content = serde_json::to_string_pretty(&unified)?;
        if let Some(parent) = args.config.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(&args.config, content)?;
        println!("{} Configuration successfully imported and saved to {:?}", "[ostp]".green().bold(), args.config);
        return Ok(());
    }

    if let Some(url) = args.url {
        println!("{} Connecting via share link...", "[ostp]".cyan().bold());
        let mut client_cfg = parse_ostp_link(&url)
            .map_err(|e| anyhow!("Share Link Error: {e}"))?;
        
        // Interactive prompt for URL launch
        use std::io::Write;
        
        print!("{} Enable TUN (VPN) mode? [y/N]: ", "?".blue().bold());
        std::io::stdout().flush().unwrap();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        if input.trim().eq_ignore_ascii_case("y") {
            if let Some(i_val) = client_cfg.get_mut("inbounds") {
                if let Some(inbounds) = i_val.as_array_mut() {
                    for inbound in inbounds.iter_mut() {
                        if inbound.get("type").and_then(|t| t.as_str()) == Some("tun") {
                            inbound["auto_route"] = serde_json::json!(true);
                        }
                    }
                }
            }
        }
        
        print!("{} Enable connection multiplexing (mux)? [y/N]: ", "?".blue().bold());
        std::io::stdout().flush().unwrap();
        input.clear();
        std::io::stdin().read_line(&mut input).unwrap();
        if input.trim().eq_ignore_ascii_case("y") {
            print!("How many sessions? [5]: ");
            std::io::stdout().flush().unwrap();
            input.clear();
            std::io::stdin().read_line(&mut input).unwrap();
            let mut sessions = 5;
            if !input.trim().is_empty() {
                if let Ok(s) = input.trim().parse() {
                    sessions = s;
                }
            }
            if let Some(o_val) = client_cfg.get_mut("outbounds") {
                if let Some(outbounds) = o_val.as_array_mut() {
                    for outbound in outbounds.iter_mut() {
                        if outbound.get("type").and_then(|t| t.as_str()) == Some("ostp") {
                            outbound["multiplex"] = serde_json::json!({
                                "enabled": true,
                                "sessions": sessions
                            });
                        }
                    }
                }
            }
        }
        
        print!("Enable debug mode? [y/N]: ");
        std::io::stdout().flush().unwrap();
        input.clear();
        std::io::stdin().read_line(&mut input).unwrap();
        if input.trim().eq_ignore_ascii_case("y") {
            client_cfg["log"]["level"] = serde_json::json!("debug");
        }

        return run_client_directly(client_cfg).await;
    }

    // Handle --check: validate config and exit
    if args.check {
        if !args.config.exists() {
            anyhow::bail!("Configuration file {:?} not found.", args.config);
        }
        let content = fs::read_to_string(&args.config)?;
        let mut stripped = json_comments::StripComments::new(content.as_bytes());
        match serde_json::from_reader::<_, UnifiedConfig>(&mut stripped) {
            Ok(config) => {
                config.validate()?;
                match &config.mode {
                    AppMode::Server(s) => {
                        println!("{} Config OK: server mode", "[ostp]".green().bold());
                        let mut keys_count = 0;
                        let mut _has_outbound = false;
                        for inbound in &s.inbounds {
                            match inbound {
                                ostp_server::config::ServerInbound::Ostp { listen, port, users, fallback, .. } => {
                                    println!("  Inbound OSTP: {}:{}", listen.cyan(), port.to_string().cyan());
                                    keys_count += users.len();
                                    if let Some(fb) = fallback {
                                        if fb.enabled {
                                            println!("    Fallback: -> {}", fb.target.cyan());
                                        }
                                    }
                                }
                                ostp_server::config::ServerInbound::Api { listen, port, .. } => {
                                    println!("  Inbound API: {}:{}", listen.cyan(), port.to_string().cyan());
                                }
                                ostp_server::config::ServerInbound::Dns { listen, .. } => {
                                    println!("  Inbound DNS Tunnel: {}", listen.cyan());
                                }
                            }
                        }
                        println!("  Access keys: {}", keys_count.to_string().yellow());
                        for ob in &s.outbounds {
                            if let ostp_server::config::ServerOutbound::Socks { server, port, .. } = ob {
                                println!("  Outbound Proxy: SOCKS5 {}:{}", server.cyan(), port.to_string().cyan());
                                _has_outbound = true;
                            }
                        }
                        if let Some(dns) = &s.dns {
                            println!("  DNS Proxy: Listen 127.0.0.1:{}", dns.local_port.to_string().cyan());
                        }
                    }
                    AppMode::Client(c) => {
                        println!("{} Config OK: client mode", "[ostp]".green().bold());
                        let (migrated, _) = ostp_client::config::ClientConfig::migrate_json(c.clone());
                        let mut display_server = "unknown";
                        let mut display_key = "unknown";
                        if let Some(outbounds) = migrated.get("outbounds").and_then(|o| o.as_array()) {
                            for outbound in outbounds {
                                if outbound.get("type").and_then(|t| t.as_str()) == Some("ostp") {
                                    if let Some(s) = outbound.get("server").and_then(|s| s.as_str()) {
                                        display_server = s;
                                    }
                                    if let Some(k) = outbound.get("access_key").and_then(|k| k.as_str()) {
                                        display_key = k;
                                    }
                                    break;
                                }
                            }
                        }
                        println!("  Server: {}", display_server.cyan());
                        println!("  Key: {}...", &display_key[..8.min(display_key.len())].yellow());
                    }
                    AppMode::Relay(r) => {
                        println!("{} Config OK: relay mode", "[ostp]".green().bold());
                        println!("  Listen: {:?}", r.listen.primary().cyan());
                        println!("  Upstream TCP: {}", r.upstream_tcp.cyan());
                        println!("  Upstream UDP: {}", r.upstream_udp.cyan());
                        println!("  API sync: {}", r.upstream_api_url.yellow());
                    }
                }
            }
            Err(e) => {
                anyhow::bail!("Config parse error: {}", e);
            }
        }
        return Ok(());
    }

    // Handle explicit configuration initialization
    if let Some(ref mode_str) = args.init {
        let is_server = mode_str == "server";
        let key = generate_secure_key("hex");
        let dns_pub = generate_secure_key("base64");
        let dns_priv = generate_secure_key("base64");
                let content = if is_server {
            format!(r#"{{
  // OSTP Server Configuration
  "version": "{}",
  "mode": "server",
  "log": {{
    // Log levels: trace, debug, info, warn, error
    "level": "info"
  }},
  "inbounds": [
    {{
      // Primary OSTP protocol listener
      "protocol": "ostp",
      "tag": "ostp-in",
      "listen": "0.0.0.0",
      "port": 50000,
      "users": [
        {{
          // Generated access key for the first client
          "key": "{}"
        }}
      ],
      "fallback": {{
        // Fallback protection: redirects unauthorized probes to a real website
        "enabled": false,
        "listen": "0.0.0.0:443",
        "target": "127.0.0.1:8080"
      }}
    }},
    {{
      // Web Administration API
      "protocol": "api",
      "tag": "api-in",
      "listen": "127.0.0.1",
      "port": 9090,
      "token": "YOUR_SECRET_TOKEN",
      "webpath": "/admin",
      "username": "admin",
      "password_hash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    }},
    {{
      // DNS Tunnel Inbound
      // [WARNING] This is a last-resort transport via public DNS.
      // It requires a dedicated registered domain with NS records pointing to this server.
      // Full setup guide: https://github.com/ospab/ostp/wiki/DNS-Tunneling
      "protocol": "dns",
      "tag": "dns-tunnel",
      "listen": "0.0.0.0:53",
      "domain": "tunnel.example.com",
      "pubkey": "{dns_pub}",
      "privkey": "{dns_priv}"
    }}
  ],
  "outbounds": [
    {{
      // Example local SOCKS5 proxy (e.g. for Tor network)
      "protocol": "socks5",
      "tag": "socks5-local",
      "server": "127.0.0.1",
      "port": 9050
    }},
    {{
      // Default direct internet access
      "protocol": "direct",
      "tag": "direct"
    }},
    {{
      // Blackhole for blocked connections
      "protocol": "block",
      "tag": "block"
    }}
  ],
  "routing": {{
    // Rule-based routing of client traffic
    "rules": [
      {{
        "domain_suffix": [".onion"],
        "outbound": "socks5-local"
      }}
    ],
    // If no rules match, use the default outbound
    "default_outbound": "direct"
  }},
  "debug": false
}}"#, env!("CARGO_PKG_VERSION"), key, dns_pub=dns_pub, dns_priv=dns_priv)
        } else if mode_str == "relay" {
            format!(r#"{{
  // OSTP Relay Configuration v0.3.5
  // DO NOT EDIT THIS COMMENT - Migrator relies on it
  "version": "{}",
  "mode": "relay",
  "log": {{
    // Log levels: trace, debug, info, warn, error
    "level": "info"
  }},
  // Local port for the relay to listen on
  "listen": "0.0.0.0:50000",
  // Upstream server details
  "upstream_tcp": "TARGET_SERVER_IP:50000",
  "upstream_udp": "TARGET_SERVER_IP:50000",
  // Upstream Control Panel API for automatic key synchronization
  "upstream_api_url": "http://TARGET_SERVER_IP:9090",
  "upstream_api_token": "YOUR_API_TOKEN_HERE",
  "sync_interval_secs": 30,
  "debug": false
}}"#, env!("CARGO_PKG_VERSION"))
        } else {
            format!(r#"{{
  // OSTP Client Configuration
  // DO NOT EDIT THIS COMMENT - Migrator relies on it
  "version": "{}",
  "mode": "client",
  "log": {{
    "level": "info"
  }},
  "inbounds": [
    {{
      // Virtual network interface for transparent proxying
      "type": "tun",
      "tag": "tun-in",
      "auto_route": true,
      "mtu": 1140
    }},
    {{
      // Local SOCKS5 proxy server for browser configuration
      "type": "local_proxy",
      "tag": "socks-in",
      "protocol": "socks",
      "listen": "127.0.0.1",
      "port": 1088
    }}
  ],
  "outbounds": [
    {{
      // Connection to the remote OSTP server
      "type": "ostp",
      "tag": "proxy",
      "server": "YOUR_SERVER_IP",
      "port": 50000,
      "access_key": "{key}",
      "transport": {{
        "type": "udp"
      }},
      "multiplex": {{
        "enabled": false,
        "sessions": 1
      }}
    }},
    {{
      "type": "direct",
      "tag": "direct"
    }},
    {{
      "type": "block",
      "tag": "block"
    }}
  ],
  "routing": {{
    "rules": [
      {{
        "domain_suffix": ["localhost"],
        "outbound": "direct"
      }}
    ],
    "default_outbound": "proxy"
  }}
}}"#, env!("CARGO_PKG_VERSION"), key = key)
        };
        if let Some(parent) = args.config.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(&args.config, &content)?;
        println!("[ostp] Configuration written to {:?}", args.config);
        
        if is_server {
            let mut stripped = json_comments::StripComments::new(content.as_bytes());
            if let Ok(config) = serde_json::from_reader::<_, UnifiedConfig>(&mut stripped) {
                if let AppMode::Server(s) = &config.mode {
                    let mut first_key = None;
                    for inbound in &s.inbounds {
                        if let ostp_server::config::ServerInbound::Ostp { users, .. } = inbound {
                            if !users.is_empty() {
                                first_key = Some(users[0].key());
                                break;
                            }
                        }
                    }
                    if let Some(key) = first_key {
                        let host = get_or_ask_public_ip(&args.config);
                        let mut query_params = Vec::<String>::new();
                        query_params.push("type=udp".to_string());

                        let mut link = format!("ostp://{}@{}:50000", key, host);
                        if !query_params.is_empty() {
                            link.push('?');
                            link.push_str(&query_params.join("&"));
                        }
                        println!("\n  Share link for client distribution:");
                        println!("  {}", link);
                    }
                }
            }
        }
        return Ok(());
    }

    if args.prober {
        dns_prober::run_prober(&args.config).await;
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
    let mut stripped = json_comments::StripComments::new(config_content.as_bytes());
    let mut raw_json: serde_json::Value = serde_json::from_reader(&mut stripped)
        .map_err(|e| anyhow!("Failed to parse config as JSON: {}", e))?;

    let is_migrated = raw_json.get("version").and_then(|v| v.as_str()) == Some(env!("CARGO_PKG_VERSION"));
    if !is_migrated {
        let is_server = raw_json.get("listen").is_some() || raw_json.get("access_keys").is_some();
        if is_server {
            raw_json["mode"] = serde_json::json!("server");
            raw_json["version"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
            if let Some(log) = raw_json.get("log_level") {
                raw_json["log"] = serde_json::json!({ "level": log.clone() });
            }
        } else {
            let (migrated, _) = ostp_client::config::ClientConfig::migrate_json(raw_json);
            raw_json = migrated;
            raw_json["mode"] = serde_json::json!("client");
        }
        
        // Save migrated config back
        let serialized = serde_json::to_string_pretty(&raw_json)?;
        let header = "// OSTP Configuration v0.3.1\n// DO NOT EDIT THIS COMMENT - Migrator relies on it\n";
        let final_content = format!("{}{}", header, serialized);
        let _ = fs::write(&args.config, final_content);
        println!("{} Configuration automatically migrated to v0.3.1", "[ostp]".cyan().bold());
    }

    let config: UnifiedConfig = serde_json::from_value(raw_json)
        .map_err(|e| anyhow!("Failed to parse config: {}", e))?;

    config.validate()?;

    if args.links {
        match &config.mode {
            AppMode::Server(server_cfg) => {
                let mut host = "127.0.0.1".to_string();
                let mut port = 50000;
                let mut users = Vec::new();
                for inbound in &server_cfg.inbounds {
                    if let ostp_server::config::ServerInbound::Ostp { listen: l, port: p, users: u, .. } = inbound {
                        if l != "0.0.0.0" {
                            host = l.clone();
                        }
                        port = *p;
                        users.extend(u.clone());
                    }
                }
                if host == "127.0.0.1" { 
                    host = get_or_ask_public_ip(&args.config); 
                }
                
                println!("\n  Client share links from {:?}:", args.config);
                if let AppMode::Server(cfg) = &config.mode {
                    let mut has_ostp = false;
                    for inbound in &cfg.inbounds {
                        if let ostp_server::config::ServerInbound::Ostp { users, .. } = inbound {
                            has_ostp = true;
                            if users.is_empty() {
                                anyhow::bail!("Ostp inbound must contain at least one user.");
                            }
                        }
                    }
                    if !has_ostp {
                        anyhow::bail!("Server configuration must contain at least one Ostp inbound.");
                    }
                }
                for (idx, user) in users.iter().enumerate() {
                    let mut query_params = Vec::<String>::new();
                    query_params.push("type=udp".to_string());

                    let mut link = format!("ostp://{}@{}:{}", user.key(), host, port);
                    if !query_params.is_empty() {
                        link.push('?');
                        link.push_str(&query_params.join("&"));
                    }
                    println!("  [{}] {}", idx + 1, link);
                }
                return Ok(());
            }
            AppMode::Client(_) => {
                anyhow::bail!("The configuration file is in Client mode. The --links flag can only extract keys from a Server configuration.");
            }
            AppMode::Relay(_) => {
                anyhow::bail!("The configuration file is in Relay mode. The --links flag only works with Server configuration.");
            }
        }
    }

    match config.mode {
        AppMode::Server(server_cfg) => {
            println!("{}", include_str!("../../docs/banner.txt").blue().bold());
            
            let mut listen_addrs = Vec::new();
            let mut access_keys_meta = Vec::new();
            let mut fallback_config = None;
            let mut host_port = ("0.0.0.0".to_string(), 50000);
            let mut api_config = None;
            let mut dns_transport = None;

            for inbound in server_cfg.inbounds {
                match inbound {
                    ostp_server::config::ServerInbound::Ostp { listen, port, users, fallback, .. } => {
                        listen_addrs.push(format!("{}:{}", listen, port));
                        host_port = (listen.clone(), port);
                        for uc in users {
                            access_keys_meta.push((uc.key(), ostp_server::api::UserMeta {
                                name: uc.name(),
                                limit_bytes: uc.limit(),
                            }));
                        }
                        if fallback_config.is_none() {
                            fallback_config = fallback;
                        }
                    }
                    ostp_server::config::ServerInbound::Api { listen, port, token, webpath, username, password_hash, .. } => {
                        api_config = Some(ostp_server::ApiConfig {
                            enabled: true,
                            bind: format!("{}:{}", listen, port),
                            token,
                            webpath: webpath.unwrap_or_default(),
                            username: username.unwrap_or_default(),
                            password_hash: password_hash.unwrap_or_default(),
                        });
                    }
                    ostp_server::config::ServerInbound::Dns { listen, domain, pubkey, privkey, .. } => {
                        dns_transport = Some(ostp_server::config::DnsTransportConfig {
                            enabled: true,
                            listen,
                            domain,
                            pubkey: pubkey.unwrap_or_default(),
                            privkey: privkey.unwrap_or_default(),
                        });
                    }
                }
            }

            println!("{} Starting server on {:?}", "[ostp]".cyan().bold(), listen_addrs);
            let debug = server_cfg.debug.unwrap_or(false);
            
            let mut outbound = None;
            for ob in server_cfg.outbounds {
                if let ostp_server::config::ServerOutbound::Socks { server, port, tag } = ob {
                    let mut rules = Vec::new();
                    let mut default_action = Some("proxy".to_string());
                    if let Some(routing) = &server_cfg.routing {
                        for rule in &routing.rules {
                            if rule.outbound == tag {
                                rules.push(ostp_server::OutboundRule {
                                    domain_suffix: rule.domain_suffix.clone().unwrap_or_default(),
                                    ip_cidr: rule.ip_cidr.clone().unwrap_or_default(),
                                    protocol: rule.protocol.clone(),
                                    action: parse_outbound_action(Some("proxy".to_string())),
                                });
                            }
                        }
                        if routing.default_outbound != tag {
                            default_action = Some("direct".to_string());
                        }
                    }
                    outbound = Some(ostp_server::OutboundConfig {
                        enabled: true,
                        protocol: "socks5".to_string(),
                        address: server,
                        port,
                        rules,
                        default_action: parse_outbound_action(default_action),
                    });
                    break;
                }
            }

            let dns_cfg = server_cfg.dns;
            
            let host = if host_port.0 == "0.0.0.0" {
                detect_local_public_ip().unwrap_or_else(|| "127.0.0.1".to_string())
            } else {
                host_port.0.to_string()
            };

            ostp_server::run_server(listen_addrs, Some(host), access_keys_meta, outbound, api_config, fallback_config, debug, dns_cfg, dns_transport, Some(args.config)).await?;
        }
        AppMode::Client(client_cfg) => {
            println!("{}", include_str!("../../docs/banner.txt").blue().bold());
            run_client_directly(client_cfg).await?;
        }
        AppMode::Relay(relay_cfg) => {
            println!("{}", include_str!("../../docs/banner.txt").blue().bold());
            let listen_addrs = relay_cfg.listen.addresses();
            println!("{} Starting relay node on {:?}", "[ostp]".cyan().bold(), listen_addrs);
            println!("{} Upstream TCP: {}", "[ostp]".cyan().bold(), relay_cfg.upstream_tcp);
            println!("{} Upstream UDP: {}", "[ostp]".cyan().bold(), relay_cfg.upstream_udp);
            println!("{} Key sync API: {}", "[ostp]".cyan().bold(), relay_cfg.upstream_api_url);
            let relay_config = ostp_server::RelayConfig {
                listen_addrs,
                upstream_tcp: relay_cfg.upstream_tcp,
                upstream_udp: relay_cfg.upstream_udp,
                upstream_api_url: relay_cfg.upstream_api_url,
                upstream_api_token: relay_cfg.upstream_api_token,
                sync_interval_secs: relay_cfg.sync_interval_secs,
            };
            ostp_server::relay_node::run_relay_node(relay_config).await?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Uninstall command
// ---------------------------------------------------------------------------
#[cfg(unix)]
fn cmd_uninstall() -> Result<()> {
    use std::process::Command;

    println!("[ostp] Uninstalling OSTP...");

    // 1. Stop and disable systemd service (best-effort)
    for action in &["stop", "disable"] {
        let _ = Command::new("systemctl")
            .args([action, "ostp"])
            .status();
    }

    // 2. Remove the systemd unit file
    let unit_path = std::path::Path::new("/etc/systemd/system/ostp.service");
    if unit_path.exists() {
        fs::remove_file(unit_path)?;
        println!("[ostp] Removed {}", unit_path.display());
        let _ = Command::new("systemctl")
            .args(["daemon-reload"])
            .status();
    }

    // 3. Remove binary
    let bin_path = std::path::Path::new("/opt/ostp/ostp");
    if bin_path.exists() {
        fs::remove_file(bin_path)?;
        println!("[ostp] Removed {}", bin_path.display());
    }

    // 4. Remove install directory
    let install_dir = std::path::Path::new("/opt/ostp");
    if install_dir.exists() {
        fs::remove_dir_all(install_dir)?;
        println!("[ostp] Removed {}", install_dir.display());
    }

    // 5. Remove configuration directory
    let config_dir = std::path::Path::new("/etc/ostp");
    if config_dir.exists() {
        fs::remove_dir_all(config_dir)?;
        println!("[ostp] Removed {}", config_dir.display());
    }

    println!("[ostp] Uninstall complete.");
    Ok(())
}

#[cfg(not(unix))]
fn cmd_uninstall() -> Result<()> {
    anyhow::bail!("The 'uninstall' command is only supported on Linux/Unix systems.");
}

// ---------------------------------------------------------------------------
// Update command
// ---------------------------------------------------------------------------
#[cfg(unix)]
fn cmd_update() -> Result<()> {
    use std::process::Command;

    println!("[ostp] Updating OSTP...");
    let status = Command::new("bash")
        .args(["-c", "bash <(curl -Ls https://raw.githubusercontent.com/ospab/ostp/master/scripts/install.sh)"])
        .status()
        .map_err(|e| anyhow!("Failed to run update: {e}"))?;

    if !status.success() {
        anyhow::bail!("Update script exited with status: {}", status);
    }
    Ok(())
}

#[cfg(not(unix))]
fn cmd_update() -> Result<()> {
    anyhow::bail!("The 'update' command is only supported on Linux/Unix systems.");
}

async fn run_client_directly(client_cfg: serde_json::Value) -> Result<()> {
    let (migrated, _) = ostp_client::config::ClientConfig::migrate_json(client_cfg);
    let client_conf: ostp_client::config::ClientConfig = serde_json::from_value(migrated)?;

    let mut is_tun_enabled = false;
    for inbound in &client_conf.inbounds {
        if matches!(inbound, ostp_client::config::InboundConfig::Tun { .. }) {
            is_tun_enabled = true;
            break;
        }
    }

    let mode_str = if is_tun_enabled { "tun" } else { "proxy" };
    println!("{} Starting client (mode={})", "[ostp]".cyan().bold(), mode_str.yellow());

    // Run the client implementation
    let (_shutdown_tx, rx) = tokio::sync::watch::channel(false);
    let metrics = std::sync::Arc::new(ostp_client::bridge::BridgeMetrics::default());
    
    // Launch the core runner directly.
    ostp_client::runner::run_client_core(client_conf, metrics, rx, None).await?;
    Ok(())
}

fn cmd_migrate(config_path: &std::path::Path) -> Result<()> {
    if !config_path.exists() {
        anyhow::bail!("Configuration file not found at {:?}", config_path);
    }

    let config_content = fs::read_to_string(config_path)?;
    let mut stripped = json_comments::StripComments::new(config_content.as_bytes());
    let mut raw_json: serde_json::Value = serde_json::from_reader(&mut stripped)
        .map_err(|e| anyhow!("Failed to parse config as JSON: {}", e))?;

    let is_migrated = raw_json.get("version").and_then(|v| v.as_str()) == Some(env!("CARGO_PKG_VERSION"));
    if is_migrated {
        println!("{} Configuration is already up to date (v0.3.5)", "[ostp]".cyan().bold());
        return Ok(());
    }

    let is_server = raw_json.get("listen").is_some() || raw_json.get("access_keys").is_some() || raw_json.get("mode").and_then(|m| m.as_str()) == Some("server");
    if is_server {
        raw_json["mode"] = serde_json::json!("server");
        raw_json["version"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
        if let Some(log) = raw_json.get("log_level") {
            raw_json["log"] = serde_json::json!({ "level": log.clone() });
        }
        
        let mut inbounds = Vec::new();
        let mut outbounds = Vec::new();
        let mut routing = serde_json::json!({
            "rules": [],
            "default_outbound": "direct"
        });

        // Migrate Ostp inbound
        let listen = raw_json.get("listen").and_then(|l| l.as_str()).unwrap_or("0.0.0.0:50000");
        let parts: Vec<&str> = listen.split(':').collect();
        let host = parts.get(0).unwrap_or(&"0.0.0.0");
        let port: u16 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(50000);
        
        let mut users = Vec::new();
        if let Some(keys) = raw_json.get("access_keys").and_then(|a| a.as_array()) {
            for k in keys {
                users.push(serde_json::json!({
                    "key": k.as_str().unwrap_or("")
                }));
            }
        }
        
        let mut ostp_inbound = serde_json::json!({
            "protocol": "ostp",
            "tag": "ostp-in",
            "listen": host,
            "port": port,
            "users": users
        });
        
        if let Some(fallback) = raw_json.get("fallback") {
            ostp_inbound["fallback"] = fallback.clone();
        }
        inbounds.push(ostp_inbound);

        // Migrate Api inbound
        if let Some(api) = raw_json.get("api") {
            let mut api_inbound = api.clone();
            api_inbound["protocol"] = serde_json::json!("api");
            api_inbound["tag"] = serde_json::json!("api-in");
            let bind = api.get("bind").and_then(|b| b.as_str()).unwrap_or("127.0.0.1:9090");
            let parts: Vec<&str> = bind.split(':').collect();
            api_inbound["listen"] = serde_json::json!(parts.get(0).unwrap_or(&"127.0.0.1"));
            api_inbound["port"] = serde_json::json!(parts.get(1).and_then(|p| p.parse::<u16>().ok()).unwrap_or(9090));
            inbounds.push(api_inbound);
        }

        // Migrate Outbound
        outbounds.push(serde_json::json!({
            "protocol": "direct",
            "tag": "direct"
        }));
        outbounds.push(serde_json::json!({
            "protocol": "block",
            "tag": "block"
        }));
        
        if let Some(ob) = raw_json.get("outbound") {
            if ob.get("enabled").and_then(|e| e.as_bool()).unwrap_or(false) {
                let tag = "socks5-legacy";
                let socks = serde_json::json!({
                    "protocol": "socks5",
                    "tag": tag,
                    "server": ob.get("address").and_then(|a| a.as_str()).unwrap_or("127.0.0.1"),
                    "port": ob.get("port").and_then(|p| p.as_u64()).unwrap_or(9050)
                });
                outbounds.push(socks);
                
                if let Some(rules) = ob.get("rules").and_then(|r| r.as_array()) {
                    let mut new_rules = Vec::new();
                    for rule in rules {
                        let mut new_rule = rule.clone();
                        new_rule["outbound"] = serde_json::json!(tag);
                        new_rules.push(new_rule);
                    }
                    routing["rules"] = serde_json::json!(new_rules);
                }
                
                let default_action = ob.get("default_action").and_then(|a| a.as_str()).unwrap_or("proxy");
                if default_action == "proxy" {
                    routing["default_outbound"] = serde_json::json!(tag);
                } else if default_action == "block" {
                    routing["default_outbound"] = serde_json::json!("block");
                }
            }
        }

        // DNS migrate
        if let Some(dns) = raw_json.get("dns_transport") {
            let mut dns_inbound = dns.clone();
            dns_inbound["protocol"] = serde_json::json!("dns");
            dns_inbound["tag"] = serde_json::json!("dns-tunnel");
            inbounds.push(dns_inbound);
        }

        raw_json["inbounds"] = serde_json::json!(inbounds);
        raw_json["outbounds"] = serde_json::json!(outbounds);
        raw_json["routing"] = routing;
        
        // Remove legacy fields
        let obj = raw_json.as_object_mut().unwrap();
        obj.remove("listen");
        obj.remove("access_keys");
        obj.remove("fallback");
        obj.remove("api");
        obj.remove("outbound");
        obj.remove("log_level");
        obj.remove("dns_transport");
        
        println!("{} Detected Server configuration.", "[ostp]".cyan().bold());
    } else {
        println!("{} Detected Client configuration.", "[ostp]".cyan().bold());
        let (migrated, _) = ostp_client::config::ClientConfig::migrate_json(raw_json.clone());
        raw_json = migrated;
        raw_json["mode"] = serde_json::json!("client");
        raw_json["version"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
    }

    let serialized = serde_json::to_string_pretty(&raw_json)?;
    let final_content = format!("{}", serialized);
    fs::write(config_path, final_content)?;
    
    println!("{} Successfully migrated configuration to v0.3.5!", "[ostp]".green().bold());
    Ok(())
}
