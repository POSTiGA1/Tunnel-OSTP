use anyhow::{anyhow, Result};
use tokio::net::TcpStream;

pub async fn dial_tcp(_target_host: &str, _target_port: u16) -> Result<TcpStream> {
    Err(anyhow!("Connection blocked by routing rule"))
}

pub async fn handle_udp(
    _client_src: std::net::SocketAddr,
    _target_dst: std::net::SocketAddr,
    _payload: bytes::Bytes,
) -> Result<()> {
    Err(anyhow!("Connection blocked by routing rule"))
}
