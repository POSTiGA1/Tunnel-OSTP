use anyhow::{anyhow, Result};
use tokio::net::TcpStream;
use crate::config::{TransportConfig, MultiplexConfig};

pub async fn dial_tcp(
    _server: &str,
    _port: u16,
    _access_key: &str,
    _transport: &TransportConfig,
    _multiplex: &MultiplexConfig,
) -> Result<TcpStream> {
    // Ostp dialer implementation.
    // For now returning an error until we migrate the local_proxy connection logic here.
    Err(anyhow!("OSTP TCP dialer not yet fully migrated"))
}

pub async fn handle_udp(
    _client_src: std::net::SocketAddr,
    _target_dst: std::net::SocketAddr,
    _payload: bytes::Bytes,
    _server: &str,
    _port: u16,
    _access_key: &str,
    _transport: &TransportConfig,
    _multiplex: &MultiplexConfig,
) -> Result<()> {
    Err(anyhow!("OSTP UDP handler not yet fully migrated"))
}
