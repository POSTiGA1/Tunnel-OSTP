use anyhow::{anyhow, Result};
#[cfg(target_os = "windows")]
use std::sync::Arc;
use tokio::sync::watch;

#[cfg(target_os = "windows")]
pub async fn run_wintun_tunnel(
    mut shutdown: watch::Receiver<bool>,
    debug: bool,
) -> Result<()> {
    if debug {
        println!("[ostp-client] Initializing Wintun adapter 'ostp_tun'...");
    }

    // 1. Load Wintun DLL
    let wintun = unsafe { wintun::load_from_path("wintun.dll") }
        .map_err(|e| anyhow!("Failed to load wintun.dll: {:?}", e))?;

    // 2. Create or Open Adapter with static name "ostp_tun"
    let adapter = match wintun::Adapter::open(&wintun, "ostp_tun") {
        Ok(a) => a,
        Err(_) => wintun::Adapter::create(&wintun, "ostp_tun", "OSTP TUN Adapter", None)
            .map_err(|e| anyhow!("Failed to create Wintun adapter: {:?}", e))?,
    };

    let adapter = Arc::new(adapter);
    
    // Set IP, Subnet and Gateway natively using netsh for bulletproof routing
    if debug {
        println!("[ostp-client] Configuring Wintun network settings via netsh...");
    }
    let output = std::process::Command::new("netsh")
        .args(["interface", "ipv4", "set", "address", "name=ostp_tun", "static", "10.1.0.2", "255.255.255.0", "10.1.0.1"])
        .output()?;
        
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("[ostp-client] Warning: netsh returned error: {}", stderr);
    } else {
        if debug {
            println!("[ostp-client] Network configured. ostp_tun IP: 10.1.0.2, Gateway: 10.1.0.1");
        }
    }

    // Start Wintun session
    let session = adapter.start_session(wintun::MAX_RING_CAPACITY)
        .map_err(|e| anyhow!("Failed to start Wintun session: {:?}", e))?;
    let session = Arc::new(session);

    if debug {
        println!("[ostp-client] TUN tunnel 'ostp_tun' is active and intercepting packets!");
    }

    // Spawn Packet Receiver Loop to read packets from Windows stack
    let rx_session = session.clone();
    tokio::task::spawn_blocking(move || {
        loop {
            match rx_session.receive_blocking() {
                Ok(packet) => {
                    let bytes = packet.bytes();
                    if bytes.len() >= 20 {
                        let proto = bytes[9];
                        let src_ip = format!("{}.{}.{}.{}", bytes[12], bytes[13], bytes[14], bytes[15]);
                        let dest_ip = format!("{}.{}.{}.{}", bytes[16], bytes[17], bytes[18], bytes[19]);
                        if debug {
                            println!("[TUN Packet] Proto={}, Src={}, Dest={}, Len={}", proto, src_ip, dest_ip, bytes.len());
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Wait for shutdown signal
    let _ = shutdown.changed().await;
    
    if debug {
        println!("[ostp-client] Shutting down Wintun adapter...");
    }
    
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub async fn run_wintun_tunnel(
    _shutdown: watch::Receiver<bool>,
    _debug: bool,
) -> Result<()> {
    Err(anyhow!("Wintun is only supported on Windows!"))
}
