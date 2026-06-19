use anyhow::Result;
use tokio::net::TcpStream;
use crate::config::{TransportConfig, MultiplexConfig};

use ostp_core::{OstpEvent, ProtocolAction, ProtocolConfig, ProtocolMachine};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Build the handshake payload the server expects:
/// [timestamp_u64_be (8 bytes)] [session_id_u32_be (4 bytes)] [access_key bytes]
fn build_handshake_payload(session_id: u32, access_key: &str) -> Vec<u8> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut payload = Vec::with_capacity(12 + access_key.len());
    payload.extend_from_slice(&ts.to_be_bytes());
    payload.extend_from_slice(&session_id.to_be_bytes());
    payload.extend_from_slice(access_key.as_bytes());
    payload
}

/// Build a correctly configured ProtocolConfig for an outgoing OSTP connection.
fn make_initiator_config(
    session_id: u32,
    access_key: &str,
    transport_cfg: &TransportConfig,
) -> ProtocolConfig {
    let secrets = ostp_core::crypto::derive_all_secrets(access_key.as_bytes());
    let payload = build_handshake_payload(session_id, access_key);
    
    let mtu = match transport_cfg.r#type.as_str() {
        "dns" => 1100,
        _ => 1350,
    };

    ProtocolConfig {
        role: ostp_core::NoiseRole::Initiator,
        psk: secrets.psk,
        session_id,
        handshake_payload: payload,
        max_padding: 256,
        padding_strategy: ostp_core::framing::PaddingStrategy::Adaptive,
        obfuscation_key: secrets.obfuscation_key,
        max_reorder: 16384,
        max_reorder_buffer: 8192,
        ack_delay_ms: 5,
        rto_ms: 100,
        max_retries: 8,
        max_sent_history: 32768,
        handshake_pad_min: secrets.handshake_pad_min,
        handshake_pad_max: secrets.handshake_pad_max,
        mtu,
    }
}

fn random_session_id() -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    std::time::Instant::now().hash(&mut h);
    std::thread::current().id().hash(&mut h);
    h.finish() as u32
}

pub async fn dial_tcp(
    server: &str,
    port: u16,
    access_key: &str,
    transport_cfg: &TransportConfig,
    _multiplex: &MultiplexConfig,
) -> Result<TcpStream> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let local_addr = listener.local_addr()?;
    let client_stream = tokio::net::TcpStream::connect(local_addr).await?;
    let (mut server_stream, _) = listener.accept().await?;

    let transport = make_transport(transport_cfg, server, port).await?;

    let session_id = random_session_id();
    let config = make_initiator_config(session_id, access_key, transport_cfg);
    let mut machine = ProtocolMachine::new(config).unwrap();

    // Spawn bridge task
    tokio::spawn(async move {
        // Send initial handshake
        if let Ok(action) = machine.on_event(OstpEvent::Start) {
            handle_action(action, &transport, &mut server_stream).await;
        }
        let mut buf = [0u8; 65535];
        let mut udp_buf = [0u8; 65535];

        loop {
            tokio::select! {
                Ok(n) = server_stream.read(&mut buf) => {
                    if n == 0 { break; }
                    if let Ok(action) = machine.on_event(OstpEvent::Outbound(1, bytes::Bytes::copy_from_slice(&buf[..n]))) {
                        handle_action(action, &transport, &mut server_stream).await;
                    }
                }
                Ok(n) = transport.recv(&mut udp_buf) => {
                    if let Ok(action) = machine.on_event(OstpEvent::Inbound(bytes::Bytes::copy_from_slice(&udp_buf[..n]))) {
                        handle_action(action, &transport, &mut server_stream).await;
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                    if let Ok(action) = machine.on_event(OstpEvent::Tick) {
                        handle_action(action, &transport, &mut server_stream).await;
                    }
                }
            }
        }
    });

    Ok(client_stream)
}

pub async fn handle_udp(
    client_src: std::net::SocketAddr,
    target_dst: std::net::SocketAddr,
    payload: bytes::Bytes,
    server: &str,
    port: u16,
    access_key: &str,
    transport_cfg: &TransportConfig,
    _multiplex: &MultiplexConfig,
) -> Result<()> {
    let transport = make_transport(transport_cfg, server, port).await?;

    // Derive session_id from client source addr for stable per-flow sessions
    let ip_bytes = match client_src.ip() {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            u32::from_be_bytes(o)
        }
        std::net::IpAddr::V6(v6) => {
            let o = v6.octets();
            u32::from_be_bytes([o[12], o[13], o[14], o[15]])
        }
    };
    let session_id = ip_bytes ^ (client_src.port() as u32);

    let config = make_initiator_config(session_id, access_key, transport_cfg);
    let mut machine = ProtocolMachine::new(config)?;

    // Send handshake first
    if let Ok(action) = machine.on_event(OstpEvent::Start) {
        handle_udp_action(action, &transport).await;
    }

    // Wait for handshake response (server sends HandshakePayload back)
    let mut buf = [0u8; 8192];
    match tokio::time::timeout(
        std::time::Duration::from_millis(2000),
        transport.recv(&mut buf),
    ).await {
        Ok(Ok(n)) => {
            let _ = machine.on_event(OstpEvent::Inbound(bytes::Bytes::copy_from_slice(&buf[..n])));
        }
        _ => {
            tracing::warn!("UDP handshake timeout for {}:{}", server, port);
            return Ok(());
        }
    }

    // Send relay connect + data
    let relay_msg = ostp_core::relay::RelayMessage::Connect(
        format!("{}:{}", target_dst.ip(), target_dst.port())
    );
    let encoded = relay_msg.encode();
    if let Ok(action) = machine.on_event(OstpEvent::Outbound(1, bytes::Bytes::from(encoded))) {
        handle_udp_action(action, &transport).await;
    }

    let data_msg = ostp_core::relay::RelayMessage::Data(payload.to_vec());
    let encoded = data_msg.encode();
    if let Ok(action) = machine.on_event(OstpEvent::Outbound(1, bytes::Bytes::from(encoded))) {
        handle_udp_action(action, &transport).await;
    }

    // Keep-alive for a short time to receive response
    for _ in 0..5 {
        match tokio::time::timeout(
            std::time::Duration::from_millis(100),
            transport.recv(&mut buf),
        ).await {
            Ok(Ok(n)) => {
                let _ = machine.on_event(OstpEvent::Inbound(bytes::Bytes::copy_from_slice(&buf[..n])));
            }
            _ => break,
        }
    }

    Ok(())
}

async fn make_transport(
    transport_cfg: &TransportConfig,
    server: &str,
    port: u16,
) -> Result<crate::transport::Transport> {
    match transport_cfg.r#type.as_str() {
        "dns" => {
            let domain = transport_cfg.domain.clone()
                .unwrap_or_else(|| "tunnel.example.com".to_string());
            let resolver = transport_cfg.resolver.clone()
                .unwrap_or_else(|| "8.8.8.8".to_string());
            let transport = crate::transport::dns::start_dns_transport(domain, resolver, transport_cfg.pubkey.clone()).await
                .map_err(|e| anyhow::anyhow!(e))?;
            Ok(transport)
        }
        _ => {
            let udp = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
            udp.connect((server, port)).await?;
            Ok(crate::transport::Transport::Udp(std::sync::Arc::new(udp)))
        }
    }
}

async fn handle_udp_action(action: ProtocolAction, transport: &crate::transport::Transport) {
    match action {
        ProtocolAction::SendDatagram(data) => {
            let _ = transport.send(&data).await;
        }
        ProtocolAction::Multiple(actions) => {
            for a in actions {
                if let ProtocolAction::SendDatagram(data) = a {
                    let _ = transport.send(&data).await;
                }
            }
        }
        _ => {}
    }
}

async fn handle_action(action: ProtocolAction, transport: &crate::transport::Transport, server_stream: &mut tokio::net::TcpStream) {
    match action {
        ProtocolAction::SendDatagram(data) => {
            let _ = transport.send(&data).await;
        }
        ProtocolAction::DeliverApp(_stream_id, payload) => {
            let _ = server_stream.write_all(&payload).await;
        }
        ProtocolAction::Multiple(actions) => {
            for a in actions {
                match a {
                    ProtocolAction::SendDatagram(data) => { let _ = transport.send(&data).await; }
                    ProtocolAction::DeliverApp(_stream_id, payload) => { let _ = server_stream.write_all(&payload).await; }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}
