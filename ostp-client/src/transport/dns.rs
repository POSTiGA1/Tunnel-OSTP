/// DNS tunnel transport — dnstt-style implementation.
///
/// Protocol (client → server, embedded in DNS query domain name):
///   Base32([client_id: 8][msg_id: 2 BE][total_frags: 1][frag_idx: 1][payload: ≤MAX_CHUNK])
///   Split into DNS labels of max 63 chars, suffixed with base_domain.
///
/// Poll query: payload is empty (total_frags=1, frag_idx=0, len=0).
///
/// Protocol (server → client, in TXT rdata):
///   Concatenated length-prefixed OSTP packets: [len: 2 BE][data ...]...
///
/// Polling: adaptive 500ms → 10s, like dnstt. Resets to 500ms on real data.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use bytes::Bytes;
use rand::Rng;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use crate::transport::Transport;
use rand::RngCore;
use ostp_core::dns::{base32_encode, DnsPacket, DnsRecordType};

/// Max raw payload bytes we put into one DNS query.
/// Calculation: FQDN ≤ 253 chars. Domain suffix ~30 chars max.
/// Remaining: ~220 chars for base32 labels. 220/8*5 = 137 bytes raw.
/// Header = 12 bytes → payload ≤ 120 bytes (conservative, works for any domain ≤ 40 chars).
const MAX_CHUNK_PAYLOAD: usize = 120;
const CLIENT_ID_LEN: usize = 8;
const INIT_POLL_DELAY: Duration = Duration::from_millis(500);
const MAX_POLL_DELAY: Duration = Duration::from_secs(10);
const POLL_DELAY_MULTIPLIER: f64 = 2.0;

pub async fn start_dns_transport(
    domain: String,
    resolver: String,
    _pubkey: Option<String>,
) -> std::io::Result<Transport> {
    let (app_tx, transport_rx) = mpsc::channel::<Bytes>(256);
    let (transport_tx, app_rx) = mpsc::channel::<Bytes>(256);

    let resolver_addr = if resolver.contains(':') {
        resolver.clone()
    } else {
        format!("{}:53", resolver)
    };

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(&resolver_addr).await?;
    let socket = Arc::new(socket);

    // Generate random ClientID for this tunnel session
    let mut client_id = [0u8; CLIENT_ID_LEN];
    rand::thread_rng().fill_bytes(&mut client_id);
    let client_id = Arc::new(client_id);

    tracing::info!("DNS transport: domain={} resolver={} client_id={}",
        domain, resolver_addr,
        hex::encode(client_id.as_slice()));

    // ── Send task ─────────────────────────────────────────────────────────────
    let sock_send = socket.clone();
    let cid_send = client_id.clone();
    let domain_send = domain.clone();
    tokio::spawn(async move {
        let mut rx = transport_rx;
        let mut msg_id: u16 = 0;
        let mut poll_delay = INIT_POLL_DELAY;

        loop {
            let data: Option<Bytes> = tokio::select! {
                data = rx.recv() => data,
                _ = tokio::time::sleep(poll_delay) => {
                    poll_delay = Duration::from_secs_f64(
                        (poll_delay.as_secs_f64() * POLL_DELAY_MULTIPLIER)
                            .min(MAX_POLL_DELAY.as_secs_f64())
                    );
                    // Send poll (empty payload)
                    Some(Bytes::new())
                }
            };

            let data = match data {
                Some(d) => d,
                None => {
                    tracing::debug!("DNS send task: channel closed, exiting");
                    break;
                }
            };

            if data.is_empty() {
                // Poll query — one empty chunk
                if let Err(e) = send_chunk(&sock_send, &cid_send, msg_id, 1, 0, &[], &domain_send).await {
                    tracing::warn!("DNS poll send error: {}", e);
                }
            } else {
                // Real OSTP packet — fragment into chunks
                poll_delay = INIT_POLL_DELAY; // reset on real data

                let data_slice = data.as_ref();
                let total_chunks = data_slice.chunks(MAX_CHUNK_PAYLOAD).count();
                let total_u8 = total_chunks.min(255) as u8;

                for (idx, chunk) in data_slice.chunks(MAX_CHUNK_PAYLOAD).enumerate() {
                    if let Err(e) = send_chunk(
                        &sock_send, &cid_send,
                        msg_id, total_u8, idx as u8,
                        chunk, &domain_send,
                    ).await {
                        tracing::warn!("DNS chunk send error (idx={}): {}", idx, e);
                        break;
                    }
                    // Brief inter-fragment delay to avoid flooding the resolver
                    if total_chunks > 1 {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                }
                msg_id = msg_id.wrapping_add(1);
            }
        }
    });

    // ── Receive task ──────────────────────────────────────────────────────────
    let sock_recv = socket.clone();
    let tx_recv = transport_tx.clone();
    let domain_recv = domain.clone();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65535];
        // Reassembly buffers: msg_id → (total, Vec<Option<chunk>>)
        let reassembly: HashMap<u16, (u8, Vec<Option<Vec<u8>>>)> = HashMap::new();

        loop {
            match sock_recv.recv(&mut buf).await {
                Ok(n) => {
                    let Some(pkt) = DnsPacket::decode(&buf[..n]) else { continue };

                    // Only process DNS responses
                    if pkt.flags & 0x8000 == 0 { continue; }

                    for answer in pkt.answers {
                        if answer.rtype != DnsRecordType::TXT && answer.rtype != DnsRecordType::NULL {
                            continue;
                        }
                        let rdata = answer.rdata;
                        // Parse length-prefixed OSTP packets packed in rdata:
                        // [len_hi: 1][len_lo: 1][data: len]...
                        let mut pos = 0;
                        while pos + 2 <= rdata.len() {
                            let pkt_len = u16::from_be_bytes([rdata[pos], rdata[pos + 1]]) as usize;
                            pos += 2;
                            if pkt_len == 0 { continue; }
                            if pos + pkt_len > rdata.len() {
                                tracing::debug!("DNS recv: truncated packet in rdata");
                                break;
                            }
                            let payload = Bytes::copy_from_slice(&rdata[pos..pos + pkt_len]);
                            pos += pkt_len;

                            if tx_recv.send(payload).await.is_err() {
                                return; // app closed
                            }
                        }
                    }

                    // Also check for responses packed in the server's extra DNS answer rdata
                    // that use our fragmentation scheme (server→client fragments)
                    // This is handled above via the length-prefix protocol.
                    let _ = &reassembly; // Keep for future upstream fragmentation support
                    let _ = &domain_recv;
                }
                Err(e) => {
                    tracing::warn!("DNS transport recv error: {}", e);
                    break;
                }
            }
        }
    });

    Ok(Transport::Dns {
        tx: app_tx,
        rx: Arc::new(Mutex::new(app_rx)),
    })
}

/// Build and send one DNS TXT query with a framed chunk.
///
/// Frame format (before base32 encoding):
///   [client_id: 8][msg_id: 2 BE][total_frags: 1][frag_idx: 1][payload: 0–120]
async fn send_chunk(
    socket: &UdpSocket,
    client_id: &[u8; CLIENT_ID_LEN],
    msg_id: u16,
    total_frags: u8,
    frag_idx: u8,
    payload: &[u8],
    base_domain: &str,
) -> std::io::Result<()> {
    // Build frame
    let mut frame = Vec::with_capacity(CLIENT_ID_LEN + 4 + payload.len());
    frame.extend_from_slice(client_id);
    frame.extend_from_slice(&msg_id.to_be_bytes());
    frame.push(total_frags);
    frame.push(frag_idx);
    frame.extend_from_slice(payload);

    // Base32-encode
    let encoded = base32_encode(&frame);

    // Split into 63-char labels and append domain
    let mut fqdn = String::with_capacity(encoded.len() + base_domain.len() + 10);
    let mut start = 0;
    while start < encoded.len() {
        let end = (start + 63).min(encoded.len());
        fqdn.push_str(&encoded[start..end]);
        fqdn.push('.');
        start = end;
    }
    fqdn.push_str(base_domain);

    // Build DNS TXT query with random ID
    let id: u16 = rand::thread_rng().gen();
    let pkt = DnsPacket::new_query(id, &fqdn, DnsRecordType::TXT);
    let wire = pkt.encode();

    tracing::trace!("DNS send chunk: msg_id={} frag={}/{} payload={}B fqdn_len={}",
        msg_id, frag_idx + 1, total_frags, payload.len(), fqdn.len());

    socket.send(&wire).await?;
    Ok(())
}
