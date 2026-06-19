use std::time::Duration;
use tokio::time::Instant;
use crate::dns::{DnsPacket, DnsRecordType, encode_payload_to_domain};
use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct DnsProbeResult {
    pub name: String,
    pub ip: String,
    pub latency_ms: Option<u64>,
}

const PUBLIC_DNS_SERVERS: &[(&str, &str)] = &[
    ("Cloudflare",  "1.1.1.1"),
    ("Cloudflare2", "1.0.0.1"),
    ("Google",      "8.8.8.8"),
    ("Google2",     "8.8.4.4"),
    ("Quad9",       "9.9.9.9"),
    ("AdGuard",     "94.140.14.14"),
    ("Yandex",      "77.88.8.8"),
    ("Yandex2",     "77.88.8.1"),
    ("SkyDNS",      "193.58.251.251"),
    ("AliDNS",      "223.5.5.5"),
    ("Tencent",     "119.29.29.29"),
    ("114DNS",      "114.114.114.114"),
    ("Shecan",      "178.22.122.100"),
    ("Electro",     "78.157.42.100"),
    ("Begzar",      "185.55.226.26"),
];

async fn probe_resolver(domain: &str, resolver_ip: &str) -> Option<u64> {
    let (probe_bytes, id) = {
        let mut rng = rand::thread_rng();
        let probe_bytes: [u8; 4] = rng.gen();
        let id: u16 = rng.gen();
        (probe_bytes, id)
    };

    let fqdn = encode_payload_to_domain(&probe_bytes, domain);
    let qtype = if rand::thread_rng().gen_bool(0.5) { DnsRecordType::TXT } else { DnsRecordType::NULL };
    let packet = DnsPacket::new_query(id, &fqdn, qtype);
    let encoded = packet.encode();

    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await.ok()?;
    sock.connect(format!("{}:53", resolver_ip)).await.ok()?;

    let start = Instant::now();
    sock.send(&encoded).await.ok()?;

    let mut buf = [0u8; 4096];
    match tokio::time::timeout(Duration::from_secs(2), sock.recv(&mut buf)).await {
        Ok(Ok(n)) => {
            if let Some(resp) = DnsPacket::decode(&buf[..n]) {
                // Check if RCODE == 0 (NOERROR) and it has answers
                let rcode = resp.flags & 0x000F;
                if rcode == 0 && !resp.answers.is_empty() {
                    return Some(start.elapsed().as_millis() as u64);
                }
            }
            None
        },
        _ => None,
    }
}

pub async fn run_dns_prober(domain: &str) -> Result<Vec<DnsProbeResult>, String> {
    if domain.is_empty() {
        return Err("Please enter the tunnel domain first (e.g. tunnel.myvpn.com)".into());
    }

    let tasks: Vec<_> = PUBLIC_DNS_SERVERS
        .iter()
        .map(|(name, ip)| {
            let domain = domain.to_string();
            let name = name.to_string();
            let ip   = ip.to_string();
            tokio::spawn(async move {
                let latency_ms = probe_resolver(&domain, &ip).await;
                DnsProbeResult { name, ip, latency_ms }
            })
        })
        .collect();

    let mut results = Vec::with_capacity(tasks.len());
    for task in tasks {
        if let Ok(r) = task.await {
            results.push(r);
        }
    }

    results.sort_by_key(|r| r.latency_ms.unwrap_or(u64::MAX));
    Ok(results)
}
