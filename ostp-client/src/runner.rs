use anyhow::Result;
use std::sync::Arc;
use tokio::sync::watch;

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

    // Report "connecting" until an inbound has successfully bound. Each inbound
    // flips this to 2 (connected) once it is ready; if they all fail, the
    // select! below returns and we reset to 0 (disconnected).
    metrics.connection_state.store(1, Ordering::Relaxed);

    let router = Arc::new(Router::new(config.routing.clone()));
    let balancer = Arc::new(Balancer::new(&config));
    
    // TODO: Detect physical interface index for bypassing
    let phys_if_for_bypass = None;
    let outbound_manager = Arc::new(OutboundManager::new(balancer.clone(), phys_if_for_bypass, None));

    let mut handles = Vec::new();

    for inbound in config.inbounds.clone() {
        let router_clone = router.clone();
        let outbound_manager_clone = outbound_manager.clone();
        let shutdown_rx = shutdown_rx_ext.clone();
        let config_clone = config.clone();
        let metrics_clone = metrics.clone();

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
                    }
                }));
            }
            InboundConfig::LocalProxy { .. } => {
                handles.push(tokio::spawn(async move {
                    if let Err(e) = crate::tunnel::inbounds::local_proxy::run_socks_inbound(
                        config_clone,
                        inbound,
                        router_clone,
                        outbound_manager_clone,
                        shutdown_rx,
                        metrics_clone,
                    ).await {
                        tracing::error!("SOCKS inbound failed: {}", e);
                    }
                }));
            }
        }
    }

    // Wait for shutdown or for tasks to fail
    tokio::select! {
        _ = shutdown_rx_ext.changed() => {
            if *shutdown_rx_ext.borrow() {
                tracing::info!("Shutdown signal received in run_client_core");
            }
        }
    }

    metrics.connection_state.store(0, Ordering::Relaxed);
    Ok(())
}
