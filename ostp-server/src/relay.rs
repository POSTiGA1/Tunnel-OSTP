use anyhow::Result;
use bytes::Bytes;
use std::collections::HashMap;

use ostp_core::relay::RelayMessage;
use tokio::io::AsyncReadExt;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::dispatcher::Dispatcher;
use crate::{RemoteState, UiEvent};

fn clean_ipv6_mapped_v4(addr: std::net::SocketAddr) -> std::net::SocketAddr {
    match addr {
        std::net::SocketAddr::V6(v6) => {
            if let Some(v4) = v6.ip().to_ipv4() {
                std::net::SocketAddr::new(std::net::IpAddr::V4(v4), v6.port())
            } else {
                addr
            }
        }
        _ => addr,
    }
}


pub async fn handle_relay_message(
    peer_addr: std::net::SocketAddr,
    session_id: u32,
    stream_id: u16,
    payload: Bytes,
    dispatcher: &mut Dispatcher,
    socket: &UdpSocket,
    remotes: &mut HashMap<(u32, u16), RemoteState>,
    ui_event_tx: &mpsc::UnboundedSender<UiEvent>,
    stream_tx: mpsc::UnboundedSender<(u32, u16, Vec<u8>)>,
    udp_reply_tx: mpsc::UnboundedSender<(u32, u16, String, Vec<u8>)>,
    connect_tx: mpsc::UnboundedSender<(u32, u16, String, Result<(tokio::net::tcp::OwnedWriteHalf, mpsc::Sender<()>), String>)>,
    router: std::sync::Arc<crate::router::Router>,
    tcp_map: &std::sync::Arc<tokio::sync::RwLock<HashMap<std::net::SocketAddr, tokio::sync::mpsc::Sender<Bytes>>>>,
) -> Result<()> {
    match RelayMessage::decode(&payload)? {
        RelayMessage::Connect(target) => {
            // DNS interception disabled for stability
            let _is_internal_dns = false;

            let mut connect_target = target.clone();
            if connect_target.starts_with("10.1.0.1:") {
                connect_target = connect_target.replace("10.1.0.1:", "127.0.0.1:");
            }

            let target_clone = connect_target.clone();
            let connect_tx_clone = connect_tx.clone();
            let stream_tx_clone = stream_tx.clone();
            let router_clone = router.clone();
            tokio::spawn(async move {
                let stream_res = router_clone.route_tcp(&target_clone).await;
                match stream_res {
                    Ok(stream) => {
                        let (mut reader, writer) = stream.into_split();
                        let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);
                        tokio::spawn(async move {
                            let mut buf = [0_u8; 4096];
                            loop {
                                tokio::select! {
                                    _ = cancel_rx.recv() => break,
                                    read_res = reader.read(&mut buf) => {
                                        match read_res {
                                            Ok(0) | Err(_) => {
                                                let _ = stream_tx_clone.send((session_id, stream_id, Vec::new()));
                                                break;
                                            }
                                            Ok(n) => {
                                                if stream_tx_clone.send((session_id, stream_id, buf[..n].to_vec())).is_err() {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        });
                        let _ = connect_tx_clone.send((session_id, stream_id, target_clone, Ok((writer, cancel_tx))));
                    }
                    Err(e) => {
                        let _ = connect_tx_clone.send((session_id, stream_id, target_clone, Err(e.to_string())));
                    }
                }
            });
        }
        RelayMessage::Data(data) => {
            if let Some(remote) = remotes.get_mut(&(session_id, stream_id)) {
                let _ = remote.data_tx.send(bytes::Bytes::from(data));
            } else {
                let _ = ui_event_tx.send(UiEvent::Log(format!("Relay DATA for unknown stream [{session_id}:{stream_id}] ({})", data.len())));
            }
        }
        RelayMessage::KeepAlive => {}
        RelayMessage::Close => {
            if let Some(state) = remotes.remove(&(session_id, stream_id)) {
                let _ = state.cancel_tx.try_send(());
                let _ = ui_event_tx.send(UiEvent::Log(format!("Relay CLOSE [{session_id}:{stream_id}]")));
            }
        }
        RelayMessage::ConnectOk => {}
        RelayMessage::Error(msg) => {
            let _ = ui_event_tx.send(UiEvent::Log(format!("Relay error from [{session_id}:{stream_id}]: {msg}")));
        }
        RelayMessage::Ping(ts) => {
            send_relay_to_stream(session_id, stream_id, RelayMessage::Pong(ts), dispatcher, socket, ui_event_tx, tcp_map).await?;
        }
        RelayMessage::Pong(_) => {}
        RelayMessage::UdpAssociate => {
            if router.debug {
                let _ = ui_event_tx.send(UiEvent::Log(format!("Relay UDP ASSOCIATE stream_id={stream_id}")));
            }
            let udp_bind_result = match UdpSocket::bind("[::]:0").await {
                Ok(s) => Ok(s),
                Err(_) => UdpSocket::bind("0.0.0.0:0").await,
            };
            let server_udp = match udp_bind_result {
                Ok(s) => std::sync::Arc::new(s),
                Err(e) => {
                    let _ = ui_event_tx.send(UiEvent::Log(format!("UDP bind failed: {e}")));
                    return Ok(());
                }
            };
            
            let session_router = std::sync::Arc::new(router.route_udp_associate(server_udp.clone()).await);

            let (udp_tx, mut udp_rx) = mpsc::unbounded_channel::<(String, Bytes)>();
            let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);
            let (dummy_data_tx, _) = mpsc::unbounded_channel::<Bytes>();

            // Outbound UDP loop (tunnel -> target)
            let tx_router = session_router.clone();
            tokio::spawn(async move {
                while let Some((target, data)) = udp_rx.recv().await {
                    let mut forward_target = target.clone();
                    if forward_target.starts_with("10.1.0.1:") {
                        forward_target = forward_target.replace("10.1.0.1:", "127.0.0.1:");
                    }
                    let _ = tx_router.send_to(&data, &forward_target).await;
                }
            });

            // Inbound UDP loop (target -> tunnel)
            let rx_sock = server_udp.clone();
            let udp_reply_clone = udp_reply_tx.clone();
            let proxy_sock = session_router.get_proxy_sock();
            tokio::spawn(async move {
                let mut direct_buf = vec![0u8; 65536];
                let mut proxy_buf = vec![0u8; 65536];
                loop {
                    if let Some(ref p) = proxy_sock {
                        tokio::select! {
                            _ = cancel_rx.recv() => break,
                            res = rx_sock.recv_from(&mut direct_buf) => {
                                if let Ok((len, addr)) = res {
                                    let _ = udp_reply_clone.send((session_id, stream_id, clean_ipv6_mapped_v4(addr).to_string(), direct_buf[..len].to_vec()));
                                } else { break; }
                            }
                            res = p.recv_from(&mut proxy_buf) => {
                                if let Ok((len, target_str)) = res {
                                    let _ = udp_reply_clone.send((session_id, stream_id, target_str, proxy_buf[..len].to_vec()));
                                } else { break; }
                            }
                        }
                    } else {
                        tokio::select! {
                            _ = cancel_rx.recv() => break,
                            res = rx_sock.recv_from(&mut direct_buf) => {
                                if let Ok((len, addr)) = res {
                                    let _ = udp_reply_clone.send((session_id, stream_id, clean_ipv6_mapped_v4(addr).to_string(), direct_buf[..len].to_vec()));
                                } else { break; }
                            }
                        }
                    }
                }
            });


            remotes.insert((session_id, stream_id), RemoteState {
                data_tx: dummy_data_tx,
                udp_tx: Some(udp_tx),
                cancel_tx,
                is_dns: false,
            });

            send_relay_to_stream(session_id, stream_id, RelayMessage::ConnectOk, dispatcher, socket, ui_event_tx, tcp_map).await?;
        }
        RelayMessage::UdpData(target, data) => {
            if let Some(remote) = remotes.get_mut(&(session_id, stream_id)) {
                // Если целевой порт 53 — пробуем перехватить через встроенный DNS
                if target.ends_with(":53") {
                    let should_intercept = {
                        let cfg = router.dns_server.config.read().await;
                        cfg.enabled || cfg.intercept_all_port53
                    };

                    if should_intercept {
                        match router.route_dns(peer_addr.ip(), &data).await {
                            Some(response) => {
                                let _ = udp_reply_tx.send((session_id, stream_id, target, response));
                                return Ok(());
                            }
                            None => {
                                // route_dns вернул None — значит DoH упал и enabled=true
                                // в режиме перехвата уже вернул SERVFAIL
                                // просто блокируем, не пускаем к 8.8.8.8 с IP сервера
                                if router.debug {
                                    let _ = ui_event_tx.send(UiEvent::Log(format!(
                                        "DNS [{session_id}:{stream_id}] DoH failed for {target}, dropping (intercept=true)"
                                    )));
                                }
                                return Ok(());
                            }
                        }
                    } else {
                        // intercept отключён: forward как обычный UDP
                        if router.debug {
                            let _ = ui_event_tx.send(UiEvent::Log(format!(
                                "DNS [{session_id}:{stream_id}] passthrough to {target} (intercept disabled)"
                            )));
                        }
                    }
                }

                if let Some(ref udp_tx) = remote.udp_tx {
                    let _ = udp_tx.send((target, Bytes::from(data)));
                }
            } else {
                let _ = ui_event_tx.send(UiEvent::Log(format!("Relay UDP DATA for unknown stream [{session_id}:{stream_id}]")));
            }
        }
    }
    Ok(())
}

pub async fn send_relay_to_stream(
    session_id: u32,
    stream_id: u16,
    msg: RelayMessage,
    dispatcher: &mut Dispatcher,
    socket: &UdpSocket,
    ui_event_tx: &mpsc::UnboundedSender<UiEvent>,
    tcp_map: &std::sync::Arc<tokio::sync::RwLock<HashMap<std::net::SocketAddr, tokio::sync::mpsc::Sender<Bytes>>>>,
) -> Result<()> {
    let payload = Bytes::from(msg.encode());
    for (frame, peer_addr) in dispatcher.outbound_to_session(session_id, stream_id, payload)? {
        let response_len = frame.len();
        let mut sent_tcp = false;
        {
            let map = tcp_map.read().await;
            if let Some(tx) = map.get(&peer_addr) {
                // Use a bounded async send with a generous timeout instead of try_send.
                // try_send silently drops frames when the channel is full (common with
                // bursty traffic), causing spurious retransmits and throughput collapse.
                // 200ms matches roughly one RTO — if we can't deliver in that window
                // the receiver is definitely stalled and we should log it.
                let tx = tx.clone();
                let frame_clone = frame.clone();
                match tokio::time::timeout(
                    std::time::Duration::from_millis(200),
                    tx.send(frame_clone),
                ).await {
                    Ok(Ok(())) => { sent_tcp = true; }
                    Ok(Err(_)) => {
                        tracing::warn!(
                            "relay: TCP channel closed for peer={}, frame dropped (session={}, stream={})",
                            peer_addr, session_id, stream_id
                        );
                        sent_tcp = true; // channel gone, don't fall through to UDP
                    }
                    Err(_timeout) => {
                        tracing::warn!(
                            "relay: TCP channel full / timeout for peer={}, falling back to UDP (session={}, stream={})",
                            peer_addr, session_id, stream_id
                        );
                        // sent_tcp stays false → will fall through to UDP send below
                    }
                }
            }
        }
        if !sent_tcp {
            let is_dns_ip = match peer_addr.ip() {
                std::net::IpAddr::V4(v4) => v4.octets()[0] == 10 && v4.octets()[1] == 255,
                _ => false,
            };
            if is_dns_ip {
                // DNS virtual IP — queue for next poll
                let mut dq = crate::dns_queue().write().await;
                let queue = dq.entry(peer_addr).or_insert_with(std::collections::VecDeque::new);
                if queue.len() < 256 {
                    queue.push_back(frame);
                } else {
                    tracing::warn!("relay: dns_queue full for peer={}, frame dropped", peer_addr);
                }
            } else {
                let _ = socket.send_to(&frame, peer_addr).await;
            }
        }
        let _ = ui_event_tx.send(UiEvent::Tx {
            peer: peer_addr.ip(),
            bytes: response_len,
        });
    }
    Ok(())
}

