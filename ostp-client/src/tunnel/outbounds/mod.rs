use anyhow::{anyhow, Result};
use std::sync::Arc;
use crate::tunnel::balancer::Balancer;
use crate::config::OutboundConfig;

pub mod direct;
pub mod block;
pub mod ostp;
pub mod socks;

pub struct OutboundManager {
    balancer: Arc<Balancer>,
    phys_if_index: Option<u32>,
    _phys_if_name: Option<String>,
}

impl OutboundManager {
    pub fn new(
        balancer: Arc<Balancer>,
        phys_if_index: Option<u32>,
        phys_if_name: Option<String>,
    ) -> Self {
        Self {
            balancer,
            phys_if_index,
            _phys_if_name: phys_if_name,
        }
    }

    pub async fn dial_tcp(&self, tag: &str, target_host: &str, target_port: u16) -> Result<tokio::net::TcpStream> {
        let concrete_config = self.balancer.get_concrete_outbound(tag)
            .ok_or_else(|| anyhow!("Outbound tag '{}' not found or resolved to invalid node", tag))?;

        match concrete_config {
            OutboundConfig::Direct { .. } => {
                direct::dial_tcp(target_host, target_port, self.phys_if_index).await
            }
            OutboundConfig::Block { .. } => {
                block::dial_tcp(target_host, target_port).await
            }
            OutboundConfig::Ostp { server, port, access_key, transport, multiplex, .. } => {
                ostp::dial_tcp(target_host, target_port, server, *port, access_key, transport, multiplex).await
            }
            OutboundConfig::Socks { server, port, .. } => {
                socks::dial_tcp(target_host, target_port, server, *port).await
            }
            _ => Err(anyhow!("Invalid concrete outbound type for {}", tag)),
        }
    }

    pub async fn handle_udp(
        &self,
        tag: &str,
        client_src: std::net::SocketAddr,
        target_dst: std::net::SocketAddr,
        payload: bytes::Bytes,
    ) -> Result<()> {
        let concrete_config = self.balancer.get_concrete_outbound(tag)
            .ok_or_else(|| anyhow!("Outbound tag '{}' not found or resolved to invalid node", tag))?;

        match concrete_config {
            OutboundConfig::Direct { .. } => {
                direct::handle_udp(client_src, target_dst, payload, self.phys_if_index).await
            }
            OutboundConfig::Block { .. } => {
                block::handle_udp(client_src, target_dst, payload).await
            }
            OutboundConfig::Ostp { server, port, access_key, transport, multiplex, .. } => {
                ostp::handle_udp(client_src, target_dst, payload, server, *port, access_key, transport, multiplex).await
            }
            OutboundConfig::Socks { server, port, .. } => {
                socks::handle_udp(client_src, target_dst, payload, server, *port).await
            }
            _ => Err(anyhow!("Invalid concrete outbound type for {}", tag)),
        }
    }
}
