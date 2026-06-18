use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, RwLock};
use std::collections::HashMap;
use std::net::SocketAddr;
use bytes::Bytes;
use tokio::time::Duration;

use ostp_core::dns::{DnsPacket, DnsRecordType, decode_domain_to_payload, encode_payload_to_domain};
use crate::config::DnsTransportConfig;
use crate::UiEvent;

pub(crate) async fn start_dns_transport_server(
    config: DnsTransportConfig,
    udp_tx: mpsc::Sender<(Bytes, SocketAddr)>,
    tcp_map: Arc<RwLock<HashMap<SocketAddr, mpsc::Sender<Bytes>>>>,
    ui_event_tx: mpsc::UnboundedSender<UiEvent>,
) {
    let listen_addr = if config.listen.contains(':') {
        config.listen.clone()
    } else {
        format!("0.0.0.0:{}", config.listen)
    };

    let socket = match UdpSocket::bind(&listen_addr).await {
        Ok(s) => Arc::new(s),
        Err(e) => {
            tracing::error!("DNS Transport failed to bind to {}: {}", listen_addr, e);
            let _ = ui_event_tx.send(UiEvent::Log(format!("DNS Transport failed to bind: {}", e)));
            return;
        }
    };

    tracing::info!("DNS Transport listening on {}", listen_addr);
    let _ = ui_event_tx.send(UiEvent::Log(format!("DNS Transport listening on {}", listen_addr)));

    let mut buf = vec![0u8; 65535];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((size, peer)) => {
                let packet_bytes = buf[..size].to_vec();
                let udp_tx = udp_tx.clone();
                let tcp_map = tcp_map.clone();
                let socket = socket.clone();
                let base_domain = config.domain.clone();

                tokio::spawn(async move {
                    if let Some(dns_req) = DnsPacket::decode(&packet_bytes) {
                        if dns_req.questions.is_empty() { return; }
                        let query = &dns_req.questions[0];
                        
                        // Check if it's our target domain and it's a TXT or NULL query
                        if (query.qtype == DnsRecordType::TXT || query.qtype == DnsRecordType::NULL) && query.name.ends_with(&base_domain) {
                            // Decode base32 payload
                            if let Some(payload) = decode_domain_to_payload(&query.name, &base_domain) {
                                
                                let (resp_tx, mut resp_rx) = mpsc::channel::<Bytes>(10);
                                
                                // Insert into tcp_map so Dispatcher routes responses to us
                                tcp_map.write().await.insert(peer, resp_tx);
                                
                                // Send payload to dispatcher
                                if udp_tx.send((Bytes::from(payload), peer)).await.is_ok() {
                                    // Wait up to 50ms for any responses
                                    let mut responses = Vec::new();
                                    
                                    while let Ok(Some(resp)) = tokio::time::timeout(Duration::from_millis(50), resp_rx.recv()).await {
                                        responses.push(resp);
                                        if responses.len() >= 3 { break; }
                                    }

                                    // Remove from tcp_map
                                    tcp_map.write().await.remove(&peer);

                                    // Build DNS Answer
                                    let mut dns_resp = DnsPacket::new_response(dns_req.id, &query.name, query.qtype.clone(), vec![]);
                                    dns_resp.answers.clear(); // We'll add our own

                                    if !responses.is_empty() {
                                        for r in responses {
                                            dns_resp.answers.push(ostp_core::dns::DnsAnswer {
                                                name: query.name.clone(),
                                                rtype: query.qtype.clone(),
                                                rclass: 1,
                                                ttl: 0,
                                                rdata: r.to_vec(),
                                            });
                                        }
                                    } else {
                                        // Empty response
                                        dns_resp.answers.push(ostp_core::dns::DnsAnswer {
                                            name: query.name.clone(),
                                            rtype: query.qtype.clone(),
                                            rclass: 1,
                                            ttl: 0,
                                            rdata: vec![],
                                        });
                                    }

                                    let resp_encoded = dns_resp.encode();
                                    let _ = socket.send_to(&resp_encoded, peer).await;
                                }
                            }
                        }
                    }
                });
            }
            Err(e) => {
                tracing::warn!("DNS Transport recv error: {}", e);
            }
        }
    }
}
