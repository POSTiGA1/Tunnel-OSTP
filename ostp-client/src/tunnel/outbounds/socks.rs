use anyhow::{anyhow, Result};
use tokio::net::TcpStream;

pub async fn dial_tcp(_target_host: &str, _target_port: u16, _server: &str, _port: u16) -> Result<TcpStream> {
    // SOCKS5 dialer implementation stub
    Err(anyhow!("SOCKS outbound TCP dialer not yet implemented"))
}

pub async fn handle_udp(
    _client_src: std::net::SocketAddr,
    _target_dst: std::net::SocketAddr,
    _payload: bytes::Bytes,
    _server: &str,
    _port: u16,
) -> Result<()> {
    Err(anyhow!("SOCKS outbound UDP handler not yet implemented"))
}
