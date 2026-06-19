use std::time::Duration;
use tokio::time::Instant;
use ostp_core::dns::{DnsPacket, DnsRecordType};
use rand::Rng;

const PUBLIC_DNS_SERVERS: &[(&str, &str)] = &[
    // --- Global & US / EU ---
    ("Google Primary", "8.8.8.8"),
    ("Google Sec", "8.8.4.4"),
    ("Cloudflare Pri", "1.1.1.1"),
    ("Cloudflare Sec", "1.0.0.1"),
    ("Quad9 Pri", "9.9.9.9"),
    ("Quad9 Sec", "149.112.112.112"),
    ("OpenDNS Pri", "208.67.222.222"),
    ("OpenDNS Sec", "208.67.220.220"),
    ("AdGuard Pri", "94.140.14.14"),
    ("AdGuard Sec", "94.140.15.15"),
    ("NextDNS", "45.90.28.0"),
    ("NextDNS Sec", "45.90.30.0"),
    ("Neustar Pri", "156.154.70.1"),
    ("Neustar Sec", "156.154.71.1"),
    ("CleanBrowsing", "185.228.168.9"),
    ("Comodo Pri", "8.26.56.26"),
    ("Comodo Sec", "8.20.247.20"),
    ("Level3 Pri", "209.244.0.3"),
    ("Level3 Sec", "209.244.0.4"),
    ("Verisign Pri", "64.6.64.6"),
    ("Verisign Sec", "64.6.65.6"),
    ("SafeDNS", "195.46.39.39"),
    ("Hurricane Pri", "74.82.42.42"),
    
    // --- Russia ---
    ("Yandex Basic", "77.88.8.8"),
    ("Yandex Basic 2", "77.88.8.1"),
    ("Yandex Safe", "77.88.8.88"),
    ("Yandex Safe 2", "77.88.8.2"),
    ("Yandex Family", "77.88.8.7"),
    ("Yandex Family 2", "77.88.8.3"),
    ("AdGuard RU Pri", "176.103.130.130"),
    ("AdGuard RU Sec", "176.103.130.131"),
    ("SkyDNS Pri", "193.58.251.251"),
    ("SkyDNS Sec", "193.58.251.252"),
    ("Rostelecom Pri", "212.48.193.36"),
    ("Rostelecom Sec", "213.134.192.222"),
    ("MTS DNS", "212.188.4.10"),
    ("Beeline DNS", "217.118.66.243"),
    ("Megafon DNS", "10.255.255.254"),
    ("TTK DNS", "217.23.136.2"),
    ("Selectel DNS", "188.128.128.128"),
    ("Selectel Sec", "188.128.128.129"),
    ("RU-CENTER", "80.252.130.254"),
    ("Mastertel", "217.70.106.5"),
    
    // --- China ---
    ("AliDNS Pri", "223.5.5.5"),
    ("AliDNS Sec", "223.6.6.6"),
    ("Tencent Pri", "119.29.29.29"),
    ("Tencent Sec", "182.254.116.116"),
    ("Baidu Pri", "180.76.76.76"),
    ("114DNS Pri", "114.114.114.114"),
    ("114DNS Sec", "114.114.115.115"),
    ("CNNIC Pri", "1.2.4.8"),
    ("CNNIC Sec", "210.2.4.8"),
    ("DNSPod Pri", "119.29.29.29"), // Same as Tencent
    ("SDNS Pri", "1.2.4.8"),        // Same as CNNIC
    ("OneDNS Pri", "117.50.11.11"),
    ("OneDNS Sec", "52.80.66.66"),
    ("CERNET Pri", "202.112.14.151"),
    ("China Telecom 1", "218.30.118.6"),
    ("China Telecom 2", "61.139.2.69"),
    ("China Unicom 1", "123.125.81.6"),
    ("China Unicom 2", "140.207.198.6"),
    ("China Mobile 1", "211.136.192.6"),
    ("China Mobile 2", "120.196.165.24"),
    
    // --- Iran ---
    ("Shecan Pri", "178.22.122.100"),
    ("Shecan Sec", "185.51.200.2"),
    ("Electro Pri", "78.157.42.100"),
    ("Electro Sec", "78.157.42.101"),
    ("Radar Pri", "10.202.10.10"),
    ("Radar Sec", "10.202.10.11"),
    ("403.online Pri", "10.202.10.202"),
    ("403.online Sec", "10.202.10.102"),
    ("Begzar Pri", "185.55.226.26"),
    ("Begzar Sec", "185.55.225.25"),
    ("Asiatech Pri", "80.253.210.253"),
    ("Asiatech Sec", "80.253.210.254"),
    ("Shatel Pri", "85.15.1.14"),
    ("Shatel Sec", "85.15.1.15"),
    ("ParsOnline", "188.122.100.100"),
    ("Irancell DNS", "109.122.192.1"),
    ("MCI DNS", "192.168.1.1"),     // Note: local router usually, but standard for MCI cellular
    ("Rightel DNS", "5.200.200.200"),
    ("Afranet Pri", "217.218.155.155"),
    ("MobinNet Pri", "5.160.0.1"),
    ("HiWeb Pri", "185.176.64.64"),
    ("Pishgaman", "5.160.25.25"),
];

pub async fn run_prober(config_path: &std::path::Path) {
    let mut target_domain = String::new();

    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(config_path) {
            let mut stripped = json_comments::StripComments::new(content.as_bytes());
            if let Ok(json_val) = serde_json::from_reader::<_, serde_json::Value>(&mut stripped) {
                // Check if it's a server config
                if let Some(inbounds) = json_val.get("inbounds").and_then(|i| i.as_array()) {
                    for inbound in inbounds {
                        if inbound.get("protocol").and_then(|p| p.as_str()) == Some("dns") {
                            if let Some(domain) = inbound.get("domain").and_then(|d| d.as_str()) {
                                target_domain = domain.to_string();
                                break;
                            }
                        }
                    }
                }
                // Check if it's a client config
                if target_domain.is_empty() {
                    if let Some(outbounds) = json_val.get("outbounds").and_then(|o| o.as_array()) {
                        for outbound in outbounds {
                            if let Some(transport) = outbound.get("transport") {
                                if transport.get("type").and_then(|t| t.as_str()) == Some("dns") {
                                    if let Some(domain) = transport.get("domain").and_then(|d| d.as_str()) {
                                        target_domain = domain.to_string();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if target_domain.is_empty() {
        println!("Could not find DNS Tunnel configuration in config.json.");
        println!("Enter your OSTP DNS Tunnel domain (e.g., tunnel.example.com):");
        std::io::stdin().read_line(&mut target_domain).unwrap();
        target_domain = target_domain.trim().to_string();
    } else {
        println!("Found DNS Tunnel domain in config.json: {}", target_domain);
    }

    if target_domain.is_empty() {
        println!("Domain cannot be empty. Exiting prober.");
        return;
    }

    println!("\nStarting DNS resolver prober for domain: {}", target_domain);
    println!("{:<15} | {:<15} | {:<10}", "Name", "IP Address", "Latency");
    println!("{:-<15}-+-{:-<15}-+-{:-<10}", "", "", "");

    let mut best_server = "8.8.8.8";
    let mut best_latency = Duration::from_secs(10);
    
    // Send a real OSTP ping packet encoded as a domain
    let payload = b"PING";
    let encoded_domain = ostp_core::dns::encode_payload_to_domain(payload, target_domain);

    let mut rng = rand::thread_rng();

    for (name, ip) in PUBLIC_DNS_SERVERS {
        let sock = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(_) => continue,
        };

        if sock.connect(format!("{}:53", ip)).await.is_err() {
            continue;
        }

        let id: u16 = rng.gen();
        let packet = DnsPacket::new_query(id, &encoded_domain, DnsRecordType::TXT);
        let payload_bytes = packet.encode();

        let start = Instant::now();
        if sock.send(&payload_bytes).await.is_ok() {
            let mut buf = [0u8; 512];
            match tokio::time::timeout(Duration::from_secs(2), sock.recv(&mut buf)).await {
                Ok(Ok(_)) => {
                    let latency = start.elapsed();
                    println!("{:<15} | {:<15} | {:<7} ms", name, ip, latency.as_millis());
                    if latency < best_latency {
                        best_latency = latency;
                        best_server = ip;
                    }
                }
                _ => {
                    println!("{:<15} | {:<15} | {:<10}", name, ip, "TIMEOUT");
                }
            }
        } else {
            println!("{:<15} | {:<15} | {:<10}", name, ip, "ERROR");
        }
    }

    println!("{:-<15}-+-{:-<15}-+-{:-<10}", "", "", "");
    println!("Best DNS Server to use for DNS Transport: {} ({} ms)", best_server, best_latency.as_millis());
    println!("Update your config.json with this resolver IP address to optimize DNS tunneling latency.");
}
