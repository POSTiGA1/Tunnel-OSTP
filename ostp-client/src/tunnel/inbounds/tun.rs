use anyhow::{anyhow, Result};
use std::sync::Arc;
use crate::config::{ClientConfig, InboundConfig};
use crate::tunnel::router::{Router, Session};
use crate::tunnel::outbounds::OutboundManager;
use tokio::sync::watch;

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub async fn run_tun_inbound(
    config: ClientConfig,
    inbound_config: InboundConfig,
    router: Arc<Router>,
    outbound_manager: Arc<OutboundManager>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    use std::net::ToSocketAddrs;
    use netstack_smoltcp::StackBuilder;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use futures::{StreamExt, SinkExt};

    let InboundConfig::Tun { tag, auto_route, mtu } = inbound_config else {
        return Err(anyhow!("Invalid config for TUN inbound"));
    };

    tracing::info!("Starting TUN inbound (tag: {}, auto_route: {}, mtu: {})", tag, auto_route, mtu);

    #[cfg(target_os = "windows")]
    let _phys_if_for_bypass: Option<u32> = ostp_tun::windows::windows_route::sys::get_default_ipv4_route().map(|(_, idx)| idx);
    #[cfg(not(target_os = "windows"))]
    let _phys_if_for_bypass: Option<u32> = None;

    let mut bypass_ips: Vec<std::net::IpAddr> = Vec::new();
    
    // Bypass all outbound server IPs
    for outbound in &config.outbounds {
        let server = match outbound {
            crate::config::OutboundConfig::Ostp { server, .. } => Some(server),
            crate::config::OutboundConfig::Socks { server, .. } => Some(server),
            _ => None,
        };
        if let Some(host) = server {
            if let Ok(ip) = host.parse::<std::net::IpAddr>() {
                bypass_ips.push(ip);
            } else {
                if let Ok(addrs) = tokio::net::lookup_host((host.as_str(), 443)).await {
                    for addr in addrs {
                        bypass_ips.push(addr.ip());
                    }
                }
            }
        }
    }

    let dummy_server_ip = bypass_ips.first().copied().unwrap_or_else(|| "8.8.8.8".parse().unwrap());

    // Create TUN device
    let opts = ostp_tun::OstpTunOptions {
        server_ip: dummy_server_ip,
        bypass_ips,
        dns_server: None,
        kill_switch: false,
        mtu: mtu as u16,
        wintun_path: None,
    };

    let tun_interface = ostp_tun::OstpTunInterface::create(opts)
        .await
        .map_err(|e| anyhow!("Failed to create OstpTunInterface: {}", e))?;
        
    let dev = tun_interface.device;
    let _route_guard = tun_interface.guard; // Drops when TUN drops

    // Build smoltcp network stack
    let (stack, tcp_runner, udp_socket, tcp_listener) = StackBuilder::default()
        .stack_buffer_size(1024)
        .tcp_buffer_size(1024)
        .udp_buffer_size(1024)
        .enable_tcp(true)
        .enable_udp(true)
        .mtu(mtu)
        .build()?;

    let mut runner_task = tokio::spawn(async move {
        if let Some(runner) = tcp_runner {
            let _ = runner.await;
        }
    });

    let (mut stack_sink, mut stack_stream) = stack.split();
    let (mut tun_read, mut tun_write) = tokio::io::split(dev);

    let mut tun_to_stack = tokio::spawn(async move {
        let mut buf = vec![0u8; 65536];
        loop {
            match tun_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let frame = buf[..n].to_vec();
                    if let Err(e) = stack_sink.send(frame).await {
                        if e.kind() == std::io::ErrorKind::BrokenPipe {
                            break;
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!("tun_read error: {e}");
                }
            }
        }
    });

    let mut stack_to_tun = tokio::spawn(async move {
        while let Some(Ok(frame)) = stack_stream.next().await {
            if let Err(e) = tun_write.write(&frame).await {
                tracing::debug!("tun_write error: {e}");
            }
        }
    });

    // ── TCP Handler ──
    let outbound_manager_tcp = outbound_manager.clone();
    let router_tcp = router.clone();
    let tag_tcp = tag.clone();
    
    let mut tcp_accept_task = tokio::spawn(async move {
        let Some(mut listener) = tcp_listener else { return; };
        while let Some((mut stream, local, remote)) = listener.next().await {
            let om = outbound_manager_tcp.clone();
            let rt = router_tcp.clone();
            let ib_tag = tag_tcp.clone();
            
            tokio::spawn(async move {
                let process_name = crate::tunnel::process_lookup::get_process_name_from_port(local.port());

                let mut sniff_buf = [0u8; 2048];
                let sniff_len = match tokio::time::timeout(
                    std::time::Duration::from_millis(100),
                    stream.read(&mut sniff_buf)
                ).await {
                    Ok(Ok(n)) => n,
                    _ => 0,
                };
                
                let mut domain_suffix = None;
                if sniff_len > 0 {
                    domain_suffix = crate::tunnel::sni_sniff::extract_sni(&sniff_buf[..sniff_len]);
                }

                let session = Session {
                    protocol: "tcp".to_string(),
                    inbound_tag: ib_tag.clone(),
                    source_ip: Some(local.ip()),
                    destination_ip: Some(remote.ip()),
                    destination_port: remote.port(),
                    sni: domain_suffix.map(|s| s.to_string()),
                    process_name,
                };

                let outbound_tag = rt.route(&session);
                tracing::info!("TUN TCP {} -> {} routed to {}", local, remote, outbound_tag);

                let target_host = if let Some(domain) = session.sni {
                    domain
                } else {
                    remote.ip().to_string()
                };

                match om.dial_tcp(&outbound_tag, &target_host, session.destination_port).await {
                    Ok(mut remote_stream) => {
                        if sniff_len > 0 {
                            if let Err(e) = remote_stream.write_all(&sniff_buf[..sniff_len]).await {
                                tracing::warn!("Failed to forward sniffed bytes to {}: {}", outbound_tag, e);
                                return;
                            }
                        }
                        let _ = tokio::io::copy_bidirectional(&mut stream, &mut remote_stream).await;
                    }
                    Err(e) => {
                        tracing::warn!("TUN TCP dial failed to {}: {}", outbound_tag, e);
                    }
                }
            });
        }
    });

    // ── UDP Handler ──
    let outbound_manager_udp = outbound_manager.clone();
    let router_udp = router.clone();
    let tag_udp = tag.clone();
    
    let mut udp_proxy_task = tokio::spawn(async move {
        if let Some(udp_sock) = udp_socket {
            let (mut udp_rx, _udp_tx) = udp_sock.split();
            while let Some((payload, local, remote)) = udp_rx.next().await {
                let process_name = crate::tunnel::process_lookup::get_process_name_from_port_udp(local.port());
                let session = Session {
                    protocol: "udp".to_string(),
                    inbound_tag: tag_udp.clone(),
                    source_ip: Some(local.ip()),
                    destination_ip: Some(remote.ip()),
                    destination_port: remote.port(),
                    sni: None,
                    process_name,
                };
                let outbound_tag = router_udp.route(&session);
                
                let payload_bytes = bytes::Bytes::copy_from_slice(&payload);
                if let Err(e) = outbound_manager_udp.handle_udp(&outbound_tag, local, remote, payload_bytes).await {
                    tracing::debug!("TUN UDP drop to {}: {}", outbound_tag, e);
                }
            }
        }
    });

    tokio::select! {
        _ = shutdown.changed() => {
            tracing::info!("TUN inbound {} shutting down", tag);
        }
        _ = &mut runner_task => {}
    }
    
    tun_to_stack.abort();
    stack_to_tun.abort();
    tcp_accept_task.abort();
    udp_proxy_task.abort();

    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub async fn run_tun_inbound(
    _config: ClientConfig,
    _inbound_config: InboundConfig,
    _router: Arc<Router>,
    _outbound_manager: Arc<OutboundManager>,
    _shutdown: watch::Receiver<bool>,
) -> Result<()> {
    Err(anyhow!("TUN is only supported on Windows and Linux"))
}
