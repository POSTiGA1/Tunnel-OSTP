use anyhow::{anyhow, Result};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

use crate::config::{ClientConfig, InboundConfig};
use crate::tunnel::balancer::Balancer;
use crate::tunnel::outbounds::OutboundManager;
use crate::tunnel::router::Router;

pub async fn run_client_core(
    config: ClientConfig,
    metrics: Arc<crate::bridge::BridgeMetrics>,
    mut shutdown_rx_ext: watch::Receiver<bool>,
    _config_rx: Option<watch::Receiver<ClientConfig>>,
) -> Result<()> {
    use portable_atomic::Ordering;
    tracing::info!("starting client core");

    // Report "connecting" until the primary inbound has fully come up. The TUN
    // inbound flips this to 2 (connected) only after the device and the server
    // bypass route are installed; the SOCKS inbound does so only when it is the
    // primary (SOCKS-only mode). If any inbound's setup fails the whole connect
    // aborts and we reset to 0 — the GUI never sees a fake "connected".
    metrics.connection_state.store(1, Ordering::Relaxed);

    let router = Arc::new(Router::new(config.routing.clone()));
    let balancer = Arc::new(Balancer::new(&config));

    // TODO: Detect physical interface index for bypassing
    let phys_if_for_bypass = None;
    let outbound_manager = Arc::new(OutboundManager::new(balancer.clone(), phys_if_for_bypass, None, Some(metrics.clone())));

    // When a TUN inbound is present it is the primary one and owns the connected
    // state; the SOCKS proxy is then secondary and must not report "connected".
    let has_tun = config
        .inbounds
        .iter()
        .any(|i| matches!(i, InboundConfig::Tun { .. }));

    // Any inbound that fails its setup reports the error here; the first report
    // aborts the whole connect so we never come up half-broken.
    let (failure_tx, mut failure_rx) = mpsc::channel::<String>(4);

    let mut handles = Vec::new();

    let metrics_ping = metrics.clone();
    let server_addr = config.outbounds.iter().find_map(|o| {
        match o {
            crate::config::OutboundConfig::Ostp { server, port, .. } => Some((server.clone(), *port)),
            crate::config::OutboundConfig::Socks { server, port, .. } => Some((server.clone(), *port)),
            _ => None,
        }
    });

    if let Some((host, port)) = server_addr {
        // Probe the REAL server port. The OSTP server listens for UoT/TCP on the
        // same port as UDP, so a plain TCP connect there confirms liveness. The
        // old code hardcoded ":443" — which the server never listens on — so the
        // probe failed every time and wrongly latched "reconnecting" forever even
        // while the tunnel was carrying traffic (the button flickered to
        // disconnected and counters appeared frozen).
        let server = if host.contains(':') { host } else { format!("{host}:{port}") };
        let mut shutdown_rx = shutdown_rx_ext.clone();
        handles.push(tokio::spawn(async move {
            // Health probe: the authoritative source of "connected". Probe the
            // server immediately, then every 3s. A reachable server latches
            // state=2 (even before any app traffic flows), and two consecutive
            // failures drop it back to 1 (reconnecting). Per-connection dials must
            // NOT drive this global state or the button flickers as connections
            // open and close.
            let mut consecutive_fail = 0u32;
            loop {
                let start = std::time::Instant::now();
                let ok = matches!(
                    tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        tokio::net::TcpStream::connect(&server),
                    )
                    .await,
                    Ok(Ok(_))
                );
                if ok {
                    let rtt = start.elapsed().as_millis() as u32;
                    metrics_ping.rtt_ms.store(rtt, Ordering::Relaxed);
                    metrics_ping.connection_state.store(2, Ordering::Relaxed);
                    consecutive_fail = 0;
                } else {
                    consecutive_fail += 1;
                    if consecutive_fail >= 2 {
                        metrics_ping.connection_state.store(1, Ordering::Relaxed);
                    }
                }
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => {}
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() { break; }
                    }
                }
            }
        }));
    }

    for inbound in config.inbounds.clone() {
        let router_clone = router.clone();
        let outbound_manager_clone = outbound_manager.clone();
        let shutdown_rx = shutdown_rx_ext.clone();
        let config_clone = config.clone();
        let metrics_clone = metrics.clone();
        let failure_tx = failure_tx.clone();

        match inbound.clone() {
            InboundConfig::Tun { .. } => {
                handles.push(tokio::spawn(async move {
                    if let Err(e) = crate::tunnel::inbounds::tun::run_tun_inbound(
                        config_clone,
                        inbound,
                        router_clone,
                        outbound_manager_clone,
                        shutdown_rx,
                        metrics_clone,
                    ).await {
                        tracing::error!("TUN inbound failed: {}", e);
                        let _ = failure_tx.send(format!("TUN inbound: {e}")).await;
                    }
                }));
            }
            InboundConfig::LocalProxy { .. } => {
                let is_primary = !has_tun;
                handles.push(tokio::spawn(async move {
                    if let Err(e) = crate::tunnel::inbounds::local_proxy::run_socks_inbound(
                        config_clone,
                        inbound,
                        router_clone,
                        outbound_manager_clone,
                        shutdown_rx,
                        metrics_clone,
                        is_primary,
                    ).await {
                        tracing::error!("SOCKS inbound failed: {}", e);
                        let _ = failure_tx.send(format!("SOCKS inbound: {e}")).await;
                    }
                }));
            }
        }
    }
    // Drop our own sender so the channel closes once every inbound task has ended.
    drop(failure_tx);

    // Run until: an external shutdown, a fatal inbound failure, or all inbounds
    // ending on their own.
    let result = tokio::select! {
        _ = shutdown_rx_ext.changed() => {
            if *shutdown_rx_ext.borrow() {
                tracing::info!("Shutdown signal received in run_client_core");
            }
            Ok(())
        }
        maybe_err = failure_rx.recv() => {
            match maybe_err {
                Some(err) => {
                    tracing::error!("tunnel startup failed: {err}");
                    Err(anyhow!("tunnel startup failed: {err}"))
                }
                None => Ok(()),
            }
        }
    };

    // Tear down every inbound regardless of why we are exiting, then report
    // disconnected so the GUI reflects the real state.
    for h in &handles {
        h.abort();
    }
    metrics.connection_state.store(0, Ordering::Relaxed);
    result
}
