use anyhow::{anyhow, Result};
use tokio::net::TcpStream;
use crate::config::{TransportConfig, MultiplexConfig};

use ostp_core::{NoiseRole, OstpEvent, ProtocolAction, ProtocolConfig, ProtocolMachine};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;

pub async fn dial_tcp(
    server: &str,
    port: u16,
    access_key: &str,
    _transport: &TransportConfig,
    _multiplex: &MultiplexConfig,
) -> Result<TcpStream> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let local_addr = listener.local_addr()?;
    let client_stream = tokio::net::TcpStream::connect(local_addr).await?;
    let (mut server_stream, _) = listener.accept().await?;

    let udp = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    udp.connect((server, port)).await?;
    
    let mut psk = [0u8; 32];
    let key_bytes = access_key.as_bytes();
    let len = key_bytes.len().min(32);
    psk[..len].copy_from_slice(&key_bytes[..len]);
    
    let config = ProtocolConfig {
        role: ostp_core::NoiseRole::Initiator,
        psk,
        session_id: 1,
        handshake_payload: vec![],
        max_padding: 0,
        padding_strategy: ostp_core::framing::PaddingStrategy::None,
        obfuscation_key: [0; 8],
        max_reorder: 16384,
        max_reorder_buffer: 8192,
        ack_delay_ms: 10,
        rto_ms: 100,
        max_retries: 5,
        max_sent_history: 32768,
        handshake_pad_min: 8,
        handshake_pad_max: 24,
        mtu: 1400,
    };
    
    let mut machine = ProtocolMachine::new(config).unwrap();
    
    // Spawn bridge task
    tokio::spawn(async move {
        if let Ok(action) = machine.on_event(OstpEvent::Start) {
            handle_action(action, &udp, &mut server_stream).await;
        }
        let mut buf = [0u8; 65535];
        let mut udp_buf = [0u8; 65535];
        
        loop {
            tokio::select! {
                Ok(n) = server_stream.read(&mut buf) => {
                    if n == 0 { break; }
                    if let Ok(action) = machine.on_event(OstpEvent::Outbound(1, bytes::Bytes::copy_from_slice(&buf[..n]))) {
                        handle_action(action, &udp, &mut server_stream).await;
                    }
                }
                Ok(n) = udp.recv(&mut udp_buf) => {
                    if let Ok(action) = machine.on_event(OstpEvent::Inbound(bytes::Bytes::copy_from_slice(&udp_buf[..n]))) {
                        handle_action(action, &udp, &mut server_stream).await;
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                    if let Ok(action) = machine.on_event(OstpEvent::Tick) {
                        handle_action(action, &udp, &mut server_stream).await;
                    }
                }
            }
        }
    });

    Ok(client_stream)
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

async fn handle_action(action: ProtocolAction, udp: &UdpSocket, server_stream: &mut tokio::net::TcpStream) {
    match action {
        ProtocolAction::SendDatagram(data) => {
            let _ = udp.send(&data).await;
        }
        ProtocolAction::DeliverApp(_stream_id, payload) => {
            let _ = server_stream.write_all(&payload).await;
        }
        ProtocolAction::Multiple(actions) => {
            for a in actions {
                match a {
                    ProtocolAction::SendDatagram(data) => { let _ = udp.send(&data).await; }
                    ProtocolAction::DeliverApp(_stream_id, payload) => { let _ = server_stream.write_all(&payload).await; }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}
