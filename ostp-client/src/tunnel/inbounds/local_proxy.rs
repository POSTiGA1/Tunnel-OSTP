use anyhow::{anyhow, Result};
use std::sync::Arc;
use crate::config::{ClientConfig, InboundConfig};
use crate::tunnel::router::{Router, Session};
use crate::tunnel::outbounds::OutboundManager;
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;

pub async fn run_socks_inbound(
    _config: ClientConfig,
    inbound_config: InboundConfig,
    router: Arc<Router>,
    outbound_manager: Arc<OutboundManager>,
    mut shutdown: watch::Receiver<bool>,
    metrics: Arc<crate::bridge::BridgeMetrics>,
) -> Result<()> {
    use portable_atomic::Ordering;
    let InboundConfig::LocalProxy { tag, protocol, listen, port, set_system_proxy } = inbound_config else {
        return Err(anyhow!("Invalid config for LocalProxy inbound"));
    };

    let bind_addr = format!("{}:{}", listen, port);
    tracing::info!("Starting {} proxy inbound on {} (tag: {})", protocol, bind_addr, tag);

    let _proxy_guard = if set_system_proxy {
        let proxy_host = if listen == "0.0.0.0" { "127.0.0.1" } else { &listen };
        Some(crate::sysproxy::SystemProxyGuard::enable(&format!("{}:{}", proxy_host, port)))
    } else {
        None
    };

    let listener = TcpListener::bind(&bind_addr).await?;

    // Listener bound successfully — the proxy is ready to accept connections.
    metrics.connection_state.store(2, Ordering::Relaxed);
    tracing::info!("{} proxy inbound ready on {}, connection state = connected", protocol, bind_addr);

    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                tracing::info!("Local proxy inbound {} shutting down", tag);
                break;
            }
            accept_res = listener.accept() => {
                if let Ok((mut stream, client_addr)) = accept_res {
                    let rt = router.clone();
                    let om = outbound_manager.clone();
                    let proto = protocol.clone();
                    let inbound_tag = tag.clone();

                    tokio::spawn(async move {
                        if proto == "socks" {
                            if let Err(e) = handle_socks5_connection(&mut stream, &rt, &om, &inbound_tag, client_addr).await {
                                tracing::debug!("SOCKS5 handling error: {}", e);
                            }
                        } else if proto == "http" {
                            if let Err(e) = handle_http_connection(&mut stream, &rt, &om, &inbound_tag, client_addr).await {
                                tracing::debug!("HTTP proxy handling error: {}", e);
                            }
                        } else {
                            tracing::error!("Unknown local proxy protocol: {}", proto);
                        }
                    });
                }
            }
        }
    }

    Ok(())
}

async fn handle_socks5_connection(
    stream: &mut tokio::net::TcpStream,
    router: &Arc<Router>,
    outbound_manager: &Arc<OutboundManager>,
    inbound_tag: &str,
    client_addr: std::net::SocketAddr,
) -> Result<()> {
    let mut buf = [0u8; 256];
    
    // Read version and method selection
    stream.read_exact(&mut buf[0..2]).await?;
    if buf[0] != 0x05 {
        return Err(anyhow!("Unsupported SOCKS version: {}", buf[0]));
    }
    
    let num_methods = buf[1] as usize;
    stream.read_exact(&mut buf[0..num_methods]).await?;
    
    // Reply with NO AUTHENTICATION REQUIRED (0x00)
    stream.write_all(&[0x05, 0x00]).await?;
    
    // Read the actual request
    stream.read_exact(&mut buf[0..4]).await?;
    if buf[0] != 0x05 || buf[1] != 0x01 { // Only CONNECT is supported
        return Err(anyhow!("Unsupported SOCKS command"));
    }
    
    let atyp = buf[3];
    let (target_host, ip_addr) = match atyp {
        0x01 => { // IPv4
            stream.read_exact(&mut buf[0..4]).await?;
            let ip = std::net::Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]);
            (ip.to_string(), Some(std::net::IpAddr::V4(ip)))
        }
        0x03 => { // Domain
            stream.read_exact(&mut buf[0..1]).await?;
            let domain_len = buf[0] as usize;
            stream.read_exact(&mut buf[0..domain_len]).await?;
            let domain = String::from_utf8_lossy(&buf[0..domain_len]).to_string();
            (domain, None)
        }
        0x04 => { // IPv6
            stream.read_exact(&mut buf[0..16]).await?;
            let mut ip_bytes = [0u8; 16];
            ip_bytes.copy_from_slice(&buf[0..16]);
            let ip = std::net::Ipv6Addr::from(ip_bytes);
            (ip.to_string(), Some(std::net::IpAddr::V6(ip)))
        }
        _ => return Err(anyhow!("Unsupported SOCKS address type: {}", atyp)),
    };
    
    stream.read_exact(&mut buf[0..2]).await?;
    let target_port = u16::from_be_bytes([buf[0], buf[1]]);
    
    let process_name = crate::tunnel::process_lookup::get_process_name_from_port(client_addr.port());

    let session = Session {
        protocol: "tcp".to_string(),
        inbound_tag: inbound_tag.to_string(),
        source_ip: Some(client_addr.ip()),
        destination_ip: ip_addr,
        destination_port: target_port,
        sni: if atyp == 0x03 { Some(target_host.clone()) } else { None },
        process_name,
    };

    let outbound_tag = router.route(&session);
    tracing::info!("SOCKS5 TCP {} -> {}:{} routed to {}", client_addr, target_host, target_port, outbound_tag);

    match outbound_manager.dial_tcp(&outbound_tag, &target_host, target_port).await {
        Ok(mut remote_stream) => {
            // Reply success
            stream.write_all(&[0x05, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).await?;
            
            // Forward data
            tokio::io::copy_bidirectional(stream, &mut remote_stream).await?;
        }
        Err(e) => {
            tracing::warn!("SOCKS5 TCP dial failed to {}: {}", outbound_tag, e);
            // Reply host unreachable
            let _ = stream.write_all(&[0x05, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).await;
        }
    }
    
    Ok(())
}

async fn handle_http_connection(
    stream: &mut tokio::net::TcpStream,
    router: &Arc<Router>,
    outbound_manager: &Arc<OutboundManager>,
    inbound_tag: &str,
    client_addr: std::net::SocketAddr,
) -> Result<()> {
    // Basic HTTP CONNECT implementation
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await?;
    if n == 0 { return Ok(()); }
    
    let request = String::from_utf8_lossy(&buf[0..n]);
    let mut lines = request.lines();
    let first_line = lines.next().ok_or_else(|| anyhow!("Empty HTTP request"))?;
    
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 3 {
        return Err(anyhow!("Invalid HTTP request line"));
    }
    
    let method = parts[0];
    let target = parts[1]; // host:port for CONNECT, http://host:port/... for GET
    
    let (target_host, target_port) = if method == "CONNECT" {
        let parts: Vec<&str> = target.split(':').collect();
        let host = parts[0].to_string();
        let port = parts.get(1).unwrap_or(&"443").parse::<u16>().unwrap_or(443);
        (host, port)
    } else {
        // Rudimentary GET parsing, ideally use httparse
        if target.starts_with("http://") {
            let without_scheme = &target[7..];
            let host_part = without_scheme.split('/').next().unwrap_or(without_scheme);
            let parts: Vec<&str> = host_part.split(':').collect();
            let host = parts[0].to_string();
            let port = parts.get(1).unwrap_or(&"80").parse::<u16>().unwrap_or(80);
            (host, port)
        } else {
            return Err(anyhow!("Unsupported HTTP method/target: {} {}", method, target));
        }
    };
    
    let process_name = crate::tunnel::process_lookup::get_process_name_from_port(client_addr.port());

    let session = Session {
        protocol: "tcp".to_string(),
        inbound_tag: inbound_tag.to_string(),
        source_ip: Some(client_addr.ip()),
        destination_ip: None, // Could parse if IP
        destination_port: target_port,
        sni: Some(target_host.clone()),
        process_name,
    };

    let outbound_tag = router.route(&session);
    tracing::info!("HTTP TCP {} -> {}:{} routed to {}", client_addr, target_host, target_port, outbound_tag);

    match outbound_manager.dial_tcp(&outbound_tag, &target_host, target_port).await {
        Ok(mut remote_stream) => {
            if method == "CONNECT" {
                stream.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;
            } else {
                remote_stream.write_all(&buf[0..n]).await?;
            }
            
            tokio::io::copy_bidirectional(stream, &mut remote_stream).await?;
        }
        Err(e) => {
            tracing::warn!("HTTP TCP dial failed to {}: {}", outbound_tag, e);
            if method == "CONNECT" {
                let _ = stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
            }
        }
    }
    
    Ok(())
}
