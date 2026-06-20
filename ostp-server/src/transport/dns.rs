/// DNS tunnel transport — dnstt-style server implementation.
///
/// Each DNS TXT query from client contains a framed chunk:
///   Base32([client_id: 8][msg_id: 2 BE][total_frags: 1][frag_idx: 1][payload: ≤120])
///
/// Server:
///   1. Decodes ClientID + fragment from query name
///   2. Reassembles fragments per (client_id, msg_id)
///   3. Forwards complete OSTP packet to dispatcher (udp_tx)
///   4. Waits up to MAX_RESPONSE_DELAY for responses
///   5. Bundles responses as length-prefixed packets in DNS TXT answer
///
/// Server → client data in TXT rdata: [len_hi][len_lo][data...]...
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, RwLock};
use tokio::time::Duration;

use ostp_core::dns::{base32_decode, DnsPacket, DnsRecordType};
use crate::config::DnsTransportConfig;
use crate::UiEvent;

const CLIENT_ID_LEN: usize = 8;
const HEADER_LEN: usize = CLIENT_ID_LEN + 4; // client_id + msg_id(2) + total(1) + idx(1)
/// How long to wait for downstream OSTP data before sending an empty response.
const MAX_RESPONSE_DELAY: Duration = Duration::from_millis(800);
/// Maximum number of response packets to bundle into one DNS answer.
const MAX_RESPONSE_PACKETS: usize = 8;
/// How long to keep per-client reassembly state without activity.
const CLIENT_EXPIRY: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct ClientId([u8; CLIENT_ID_LEN]);

struct ReassemblyState {
    total: u8,
    frags: Vec<Option<Vec<u8>>>,
    received: u8,
}

impl ReassemblyState {
    fn new(total: u8) -> Self {
        Self {
            total,
            frags: vec![None; total as usize],
            received: 0,
        }
    }

    fn insert(&mut self, idx: u8, payload: Vec<u8>) -> bool {
        let idx = idx as usize;
        if idx >= self.frags.len() { return false; }
        if self.frags[idx].is_none() {
            self.frags[idx] = Some(payload);
            self.received += 1;
        }
        self.received >= self.total
    }

    fn assemble(self) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        for frag in self.frags {
            out.extend_from_slice(&frag?);
        }
        Some(out)
    }
}

struct ClientState {
    /// msg_id → reassembly buffer
    reassembly: HashMap<u16, ReassemblyState>,
    /// Channel to push pending responses into; DNS handler polls this per-query
    #[allow(dead_code)]
    resp_tx: mpsc::Sender<Bytes>,
    last_seen: std::time::Instant,
}

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

    // Per-client state: ClientId → ClientState
    // Access is serialised by a single Mutex so fragments from the same client
    // are always reassembled atomically.
    let clients: Arc<tokio::sync::Mutex<HashMap<ClientId, ClientState>>> =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    // Cleanup task: evict stale client state
    {
        let clients_gc = clients.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                let mut map = clients_gc.lock().await;
                map.retain(|_, v| v.last_seen.elapsed() < CLIENT_EXPIRY);
            }
        });
    }

    let base_domain = config.domain.clone();
    let mut buf = vec![0u8; 65535];

    loop {
        let (size, peer) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("DNS Transport recv error: {}", e);
                continue;
            }
        };

        let packet_bytes = buf[..size].to_vec();
        let udp_tx = udp_tx.clone();
        let tcp_map = tcp_map.clone();
        let socket = socket.clone();
        let clients = clients.clone();
        let base_domain = base_domain.clone();

        tokio::spawn(async move {
            handle_dns_query(
                packet_bytes, peer,
                udp_tx, tcp_map, socket, clients, base_domain,
            ).await;
        });
    }
}

async fn handle_dns_query(
    packet_bytes: Vec<u8>,
    peer: SocketAddr,
    udp_tx: mpsc::Sender<(Bytes, SocketAddr)>,
    tcp_map: Arc<RwLock<HashMap<SocketAddr, mpsc::Sender<Bytes>>>>,
    socket: Arc<UdpSocket>,
    clients: Arc<tokio::sync::Mutex<HashMap<ClientId, ClientState>>>,
    base_domain: String,
) {
    let dns_req = match DnsPacket::decode(&packet_bytes) {
        Some(p) => p,
        None => {
            tracing::debug!("DNS: failed to decode packet from {}", peer);
            return;
        }
    };

    if dns_req.questions.is_empty() { return; }
    let query = &dns_req.questions[0];

    // Must be TXT query for our subdomain
    if query.qtype != DnsRecordType::TXT && query.qtype != DnsRecordType::NULL { return; }
    if !query.name.ends_with(&base_domain) { return; }

    // Strip base domain and labels separator to get base32 subdomain
    let subdomain = {
        let name_lower = query.name.to_lowercase();
        let suffix = format!(".{}", base_domain.to_lowercase());
        let suffix_bare = base_domain.to_lowercase();
        let stripped = if name_lower.ends_with(&suffix) {
            &query.name[..name_lower.len() - suffix.len()]
        } else if name_lower == suffix_bare {
            ""
        } else {
            return;
        };
        // Remove dots (label separators) to get contiguous base32
        stripped.replace('.', "")
    };

    if subdomain.is_empty() {
        // Pure poll — no payload
        let resp = build_dns_response(&dns_req, &query.name, query.qtype.clone(), vec![]);
        let _ = socket.send_to(&resp, peer).await;
        return;
    }

    // Base32-decode
    let raw = match base32_decode(&subdomain) {
        Some(b) => b,
        None => {
            tracing::debug!("DNS: base32 decode failed from {}", peer);
            return;
        }
    };

    if raw.len() < HEADER_LEN {
        tracing::debug!("DNS: frame too short ({} bytes) from {}", raw.len(), peer);
        return;
    }

    // Parse header
    let client_id = ClientId(raw[..CLIENT_ID_LEN].try_into().unwrap());
    let msg_id = u16::from_be_bytes([raw[8], raw[9]]);
    let total_frags = raw[10];
    let frag_idx = raw[11];
    let payload = raw[HEADER_LEN..].to_vec();

    tracing::trace!("DNS: client={} msg={} frag={}/{} payload={}B",
        hex::encode(&client_id.0), msg_id, frag_idx + 1, total_frags, payload.len());

    // ── Reassembly ────────────────────────────────────────────────────────────
    let complete_packet: Option<Vec<u8>> = {
        let mut map = clients.lock().await;
        let state = map.entry(client_id).or_insert_with(|| {
            let (resp_tx, _) = mpsc::channel(64); // placeholder, will be replaced below
            ClientState {
                reassembly: HashMap::new(),
                resp_tx,
                last_seen: std::time::Instant::now(),
            }
        });
        state.last_seen = std::time::Instant::now();

        if total_frags == 0 {
            // Empty poll — no data
            None
        } else if total_frags == 1 && payload.is_empty() {
            // Poll with empty payload
            None
        } else {
            let asm = state.reassembly
                .entry(msg_id)
                .or_insert_with(|| ReassemblyState::new(total_frags));

            if asm.insert(frag_idx, payload) {
                // All fragments received — assemble and remove
                let complete = state.reassembly.remove(&msg_id)
                    .and_then(|s| s.assemble());
                complete
            } else {
                None
            }
        }
    };

    // ── Create per-query response channel ────────────────────────────────────
    // We use the peer SocketAddr as the routing key in tcp_map.
    // For each query we create a fresh one-shot channel.
    let (resp_tx, mut resp_rx) = mpsc::channel::<Bytes>(MAX_RESPONSE_PACKETS);
    tcp_map.write().await.insert(peer, resp_tx);

    // ── Forward complete OSTP packet to dispatcher ────────────────────────────
    if let Some(ostp_pkt) = complete_packet {
        tracing::debug!("DNS: forwarding {}B OSTP packet from client={} to dispatcher",
            ostp_pkt.len(), hex::encode(&client_id.0));
        let _ = udp_tx.send((Bytes::from(ostp_pkt), peer)).await;
    }

    // ── Wait for OSTP response(s) ─────────────────────────────────────────────
    let mut responses: Vec<Bytes> = Vec::new();
    let deadline = tokio::time::sleep(MAX_RESPONSE_DELAY);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = &mut deadline => break,
            resp = resp_rx.recv() => {
                match resp {
                    Some(r) => {
                        responses.push(r);
                        if responses.len() >= MAX_RESPONSE_PACKETS { break; }
                    }
                    None => break,
                }
            }
        }
    }

    tcp_map.write().await.remove(&peer);

    // ── Build DNS TXT response ────────────────────────────────────────────────
    // Bundle all response packets as length-prefixed data in TXT rdata:
    // [len_hi][len_lo][data...]...
    let mut rdata: Vec<u8> = Vec::new();
    for r in &responses {
        let len = r.len() as u16;
        rdata.push((len >> 8) as u8);
        rdata.push((len & 0xFF) as u8);
        rdata.extend_from_slice(r);
    }

    tracing::trace!("DNS: responding to {} with {} OSTP packets ({} bytes rdata)",
        peer, responses.len(), rdata.len());

    let resp = build_dns_response(&dns_req, &query.name, query.qtype.clone(), rdata);
    let _ = socket.send_to(&resp, peer).await;
}

/// Build a DNS response packet with the given TXT rdata.
fn build_dns_response(
    req: &DnsPacket,
    name: &str,
    rtype: DnsRecordType,
    rdata: Vec<u8>,
) -> Vec<u8> {
    let resp = DnsPacket::new_response(req.id, name, rtype, rdata);
    resp.encode()
}
