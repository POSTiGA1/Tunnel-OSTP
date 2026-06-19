use std::sync::Arc;
use std::time::Duration;
use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use rand::Rng;

pub use ostp_core::dns::{
    DnsPacket, DnsRecordType, encode_payload_to_domain,
    decode_domain_to_payload,
};
use crate::transport::Transport;

pub async fn start_dns_transport(domain: String, resolver: String, _pubkey: Option<String>) -> std::io::Result<Transport> {
    let (app_tx, transport_rx) = mpsc::channel::<Bytes>(100);
    let (transport_tx, app_rx) = mpsc::channel::<Bytes>(100);

    let resolver_addr = if resolver.contains(':') {
        resolver.clone()
    } else {
        format!("{}:53", resolver)
    };

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(&resolver_addr).await?;
    let socket = Arc::new(socket);

    let sock_rx = socket.clone();
    let sock_tx = socket;
    let base_domain = domain.clone();

    // Send task (reads from app, encodes into DNS TXT, sends to UDP socket)
    tokio::spawn(async move {
        let mut rx = transport_rx;
        loop {
            let data_opt = tokio::select! {
                res = rx.recv() => res,
                _ = tokio::time::sleep(Duration::from_secs(2)) => Some(Bytes::new()),
            };
            
            let data = match data_opt {
                Some(d) => d,
                None => break, // App closed
            };

            // Encode data to base32 domain
            let fqdn = encode_payload_to_domain(&data, &base_domain);
            let id: u16 = rand::thread_rng().gen();
            
            // Randomly choose TXT or NULL for diversity (as requested)
            let qtype = if rand::thread_rng().gen_bool(0.5) {
                DnsRecordType::TXT
            } else {
                DnsRecordType::NULL
            };

            let packet = DnsPacket::new_query(id, &fqdn, qtype);
            let encoded = packet.encode();

            if let Err(e) = sock_tx.send(&encoded).await {
                tracing::warn!("DNS transport send error: {}", e);
                break;
            }
        }
    });

    // Receive task (reads from UDP socket, decodes DNS answer, sends to app)
    let _base_domain_rx = domain.clone();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65535];
        loop {
            match sock_rx.recv(&mut buf).await {
                Ok(n) => {
                    if let Some(packet) = DnsPacket::decode(&buf[..n]) {
                        for answer in packet.answers {
                            if answer.rtype == DnsRecordType::TXT || answer.rtype == DnsRecordType::NULL {
                                // If it's a TXT record, the response might be base32 encoded payload?
                                // Actually, dnstt puts the payload in the TXT/NULL record data.
                                // We'll just assume the rdata is the raw payload, or base32 encoded if it was sent as such.
                                // Let's just pass the raw data (TXT strings are decoded in DnsPacket::decode)
                                
                                // Wait, dnstt server responds with raw bytes in NULL, and base32/chunked strings in TXT.
                                // Our `DnsPacket::decode` already handles extracting TXT string bytes or NULL raw bytes into `rdata`.
                                // Let's just send `rdata` to the app.
                                if transport_tx.send(Bytes::from(answer.rdata)).await.is_err() {
                                    return; // App closed
                                }
                            }
                        }
                    }
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
