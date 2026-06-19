use anyhow::Result;
use tokio::net::TcpStream;
use crate::config::{TransportConfig, MultiplexConfig};

use ostp_core::{OstpEvent, ProtocolAction, ProtocolConfig, ProtocolMachine};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

    let transport = match transport_cfg.r#type.as_str() {
        "dns" => {
            let domain = transport_cfg.domain.clone().unwrap_or_else(|| "tunnel.example.com".to_string());
            let resolver = transport_cfg.resolver.clone().unwrap_or_else(|| "8.8.8.8".to_string());
            crate::transport::dns::start_dns_transport(domain, resolver, transport_cfg.pubkey.clone()).await?
        }
        // Fallback to UDP for now if unknown
        _ => {
            let udp = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
            udp.connect((server, port)).await?;
            crate::transport::Transport::Udp(std::sync::Arc::new(udp))
        }
    };
    
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
        padding_strategy: ostp_core::framing::PaddingStrategy::Fixed(0),
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
    let transport = match transport_cfg.r#type.as_str() {
        "dns" => {
            let domain = transport_cfg.domain.clone().unwrap_or_else(|| "tunnel.example.com".to_string());
            let resolver = transport_cfg.resolver.clone().unwrap_or_else(|| "8.8.8.8".to_string());
            crate::transport::dns::start_dns_transport(domain, resolver, transport_cfg.pubkey.clone()).await?
        }
        _ => {
            let udp = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
            udp.connect((server, port)).await?;
            crate::transport::Transport::Udp(std::sync::Arc::new(udp))
        }
    };

    let mut psk = [0u8; 32];
    let key_bytes = access_key.as_bytes();
    let len = key_bytes.len().min(32);
    psk[..len].copy_from_slice(&key_bytes[..len]);

    let config = ProtocolConfig {
        role: ostp_core::NoiseRole::Initiator,
        psk,
        session_id: u32::from_ne_bytes([
            client_src.ip().to_string().as_bytes().get(0).copied().unwrap_or(0),
            client_src.ip().to_string().as_bytes().get(1).copied().unwrap_or(0),
            client_src.ip().to_string().as_bytes().get(2).copied().unwrap_or(0),
            client_src.ip().to_string().as_bytes().get(3).copied().unwrap_or(0),
        ]),
        handshake_payload: vec![],
        max_padding: 0,
        padding_strategy: ostp_core::framing::PaddingStrategy::Fixed(0),
        obfuscation_key: [0; 8],
        max_reorder: 4096,
        max_reorder_buffer: 2048,
        ack_delay_ms: 50,
        rto_ms: 200,
        max_retries: 3,
        max_sent_history: 8192,
        handshake_pad_min: 8,
        handshake_pad_max: 24,
        mtu: 1400,
    };

    let mut machine = ProtocolMachine::new(config)?;

    // Send initial packet with UDP payload
    if let Ok(action) = machine.on_event(OstpEvent::Start) {
        handle_udp_action(action, &transport).await;
    }

    // Send the actual UDP payload
    let relay_msg = ostp_core::relay::RelayMessage::Connect(format!("{}:{}", target_dst.ip(), target_dst.port()));
    let encoded = relay_msg.encode();
    if let Ok(action) = machine.on_event(OstpEvent::Outbound(1, bytes::Bytes::from(encoded))) {
        handle_udp_action(action, &transport).await;
    }

    // Send data packet
    let data_msg = ostp_core::relay::RelayMessage::Data(payload.to_vec());
    let encoded = data_msg.encode();
    if let Ok(action) = machine.on_event(OstpEvent::Outbound(1, bytes::Bytes::from(encoded))) {
        handle_udp_action(action, &transport).await;
    }

    // Keep-alive for a short time to receive response
    for _ in 0..5 {
        let mut buf = [0u8; 8192];
        match tokio::time::timeout(
            std::time::Duration::from_millis(100),
            transport.recv(&mut buf)
        ).await {
            Ok(Ok(n)) => {
                let _ = machine.on_event(OstpEvent::Inbound(bytes::Bytes::copy_from_slice(&buf[..n])));
            }
            _ => break,
        }
    }

    Ok(())
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
