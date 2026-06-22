use std::env;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use anyhow::{Context, Result};
use tracing::{debug, info};

pub struct DnsttProcess {
    child: Child,
}

impl Drop for DnsttProcess {
    fn drop(&mut self) {
        debug!("Stopping dnstt process...");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn find_bin(name: &str) -> Result<PathBuf> {
    let ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    let file_name = format!("{}{}", name, ext);
    
    // Check current working directory
    let mut path = env::current_dir()?.join(&file_name);
    if path.exists() {
        return Ok(path);
    }
    
    // Check next to the executable
    if let Ok(exe_path) = env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            path = parent.join(&file_name);
            if path.exists() {
                return Ok(path);
            }
        }
    }
    
    anyhow::bail!("{} not found. Please place {} in the same directory as ostp.", name, file_name);
}

/// Spawns the dnstt-server process.
/// Listens on `public_ip:53` and forwards to `127.0.0.1:<local_port>`.
pub fn spawn_server(public_ip: &str, local_port: u16, privkey: &str, debug: bool) -> Result<DnsttProcess> {
    let bin_path = find_bin("dnstt-server")?;
    let listen_addr = format!("{}:53", public_ip);
    let forward_addr = format!("127.0.0.1:{}", local_port);
    
    info!("Starting dnstt-server on {} forwarding to {}", listen_addr, forward_addr);
    
    let child = Command::new(&bin_path)
        .arg("-udp")
        .arg(&listen_addr)
        .arg("-privkey")
        .arg(privkey)
        .arg(&forward_addr)
        .stdout(if debug { Stdio::inherit() } else { Stdio::null() })
        .stderr(if debug { Stdio::inherit() } else { Stdio::null() })
        .spawn()
        .context("Failed to start dnstt-server process")?;
        
    Ok(DnsttProcess { child })
}

/// Spawns the dnstt-client process.
/// Returns the local port it bound to, along with the process handle.
pub fn spawn_client(pubkey: &str, domain: &str, resolver: &str, debug: bool) -> Result<(u16, DnsttProcess)> {
    let bin_path = find_bin("dnstt-client")?;
    
    let local_port = {
        let listener = std::net::UdpSocket::bind("127.0.0.1:0")?;
        listener.local_addr()?.port()
    };
    
    let listen_addr = format!("127.0.0.1:{}", local_port);
    info!("Starting dnstt-client on {} via {}", listen_addr, resolver);
    
    let child = Command::new(&bin_path)
        .arg("-udp")
        .arg(resolver)
        .arg("-pubkey")
        .arg(pubkey)
        .arg(domain)
        .arg(&listen_addr)
        .stdout(if debug { Stdio::inherit() } else { Stdio::null() })
        .stderr(if debug { Stdio::inherit() } else { Stdio::null() })
        .spawn()
        .context("Failed to start dnstt-client process")?;
        
    Ok((local_port, DnsttProcess { child }))
}

/// Helper to generate a new keypair using dnstt-server -gen-key
pub fn generate_keypair() -> Result<(String, String)> {
    let bin_path = find_bin("dnstt-server")?;
    
    let output = Command::new(&bin_path)
        .arg("-gen-key")
        .output()
        .context("Failed to run dnstt-server -gen-key")?;
        
    if !output.status.success() {
        anyhow::bail!("dnstt-server -gen-key failed");
    }
    
    let out_str = String::from_utf8_lossy(&output.stdout);
    let mut privkey = String::new();
    let mut pubkey = String::new();
    
    for line in out_str.lines() {
        if line.starts_with("privkey ") {
            privkey = line.trim_start_matches("privkey ").trim().to_string();
        } else if line.starts_with("pubkey ") {
            pubkey = line.trim_start_matches("pubkey ").trim().to_string();
        }
    }
    
    if privkey.is_empty() || pubkey.is_empty() {
        anyhow::bail!("Failed to parse keys from dnstt-server output");
    }
    
    Ok((privkey, pubkey))
}
