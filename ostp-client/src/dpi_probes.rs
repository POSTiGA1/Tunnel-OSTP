//! Generic (non-ostp-specific) DPI/TSPU fingerprinting, ported from the
//! standalone `ostp-prober` desktop tool's `probes/dpi.rs` — the same raw
//! TCP/UDP differential tests, same target hosts and block-page signatures,
//! so a report from this module reads the same way as one from the desktop
//! tool. Where `prober.rs` in this crate answers "does *my* ostp server work
//! from here", this module answers "what does this network filter in
//! general" — useful context when the former fails: is it the network, or
//! just this server/transport?
//!
//! Every socket here is opened through [`protected_tcp_connect`] /
//! [`protected_udp_socket`], which calls [`crate::bridge::protect_socket`]
//! on the raw fd before it touches the network. Without that, running this
//! battery while the ostp VPN tunnel is active would route these probes
//! through the tunnel itself and always report a clean network — exactly
//! the case this module exists to diagnose.
//!
//! No stubbed results: a test that can't run on this platform/permission
//! level is simply not run, never faked.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpSocket, TcpStream, UdpSocket};

// Cross-SNI targets: (IP, native SNI for that IP, port). Stable Russian IPs
// that answer their own SNI — same pair the desktop tool uses, so results
// from both tools are directly comparable.
const CROSS_SNI_TARGETS: &[(&str, &str, u16)] = &[
    ("87.240.132.78", "vk.com", 443), // VKontakte
    ("77.88.55.242", "ya.ru", 443),   // Yandex
];

const BLOCKED_SNIS: &[&str] = &["instagram.com", "twitter.com", "facebook.com"];
const BLOCKED_DOMAINS: &[&str] = &["instagram.com", "twitter.com", "facebook.com"];

// ── Protected socket primitives ─────────────────────────────────────────────

async fn protected_tcp_connect(addr: SocketAddr, timeout: Duration) -> std::io::Result<TcpStream> {
    let socket = if addr.is_ipv6() { TcpSocket::new_v6()? } else { TcpSocket::new_v4()? };
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        crate::bridge::protect_socket(socket.as_raw_fd());
    }
    match tokio::time::timeout(timeout, socket.connect(addr)).await {
        Ok(r) => r,
        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "connect timed out")),
    }
}

async fn protected_udp_socket(v6: bool) -> std::io::Result<UdpSocket> {
    let domain = if v6 { socket2::Domain::IPV6 } else { socket2::Domain::IPV4 };
    let sock = socket2::Socket::new(domain, socket2::Type::DGRAM, Some(socket2::Protocol::UDP))?;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        crate::bridge::protect_socket(sock.as_raw_fd());
    }
    let bind_addr: SocketAddr = if v6 {
        (IpAddr::from(std::net::Ipv6Addr::UNSPECIFIED), 0).into()
    } else {
        (IpAddr::from(Ipv4Addr::UNSPECIFIED), 0).into()
    };
    sock.bind(&bind_addr.into())?;
    sock.set_nonblocking(true)?;
    UdpSocket::from_std(sock.into())
}

// ── Probe primitives ─────────────────────────────────────────────────────────

enum ProbeOutcome {
    ServerResponded,
    FastReset,
    SlowClose,
    Dropped,
}

fn is_blocked(outcome: &ProbeOutcome) -> bool {
    matches!(outcome, ProbeOutcome::FastReset | ProbeOutcome::Dropped)
}

async fn measure_rtt(ip: &str, port: u16) -> Option<u64> {
    let addr: SocketAddr = format!("{ip}:{port}").parse().ok()?;
    let mut samples: Vec<u64> = Vec::with_capacity(4);
    for _ in 0..4 {
        let t = Instant::now();
        if protected_tcp_connect(addr, Duration::from_millis(3000)).await.is_ok() {
            samples.push(t.elapsed().as_millis() as u64);
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    if samples.len() < 2 {
        return None;
    }
    samples.sort_unstable();
    Some(samples[samples.len() / 2])
}

async fn probe_tls(ip: &str, port: u16, sni: &str, rtt: u64) -> ProbeOutcome {
    let timeout_ms = (rtt * 5).max(2000);
    let Ok(addr) = format!("{ip}:{port}").parse::<SocketAddr>() else { return ProbeOutcome::Dropped };
    let t = Instant::now();

    let mut stream = match protected_tcp_connect(addr, Duration::from_millis(timeout_ms)).await {
        Ok(s) => s,
        Err(_) => return ProbeOutcome::FastReset,
    };

    let hello = build_tls_client_hello(sni);
    if stream.write_all(&hello).await.is_err() {
        return ProbeOutcome::FastReset;
    }

    let mut buf = [0u8; 128];
    match tokio::time::timeout(Duration::from_millis(timeout_ms), stream.read(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => ProbeOutcome::ServerResponded,
        Ok(Ok(0)) | Ok(Err(_)) => {
            let elapsed = t.elapsed().as_millis() as u64;
            if elapsed < rtt / 2 { ProbeOutcome::FastReset } else { ProbeOutcome::SlowClose }
        }
        _ => ProbeOutcome::Dropped,
    }
}

async fn probe_http(ip: &str, host: &str, rtt: u64) -> ProbeOutcome {
    let timeout_ms = (rtt * 5).max(2000);
    let Ok(addr) = format!("{ip}:80").parse::<SocketAddr>() else { return ProbeOutcome::Dropped };
    let t = Instant::now();

    let mut stream = match protected_tcp_connect(addr, Duration::from_millis(timeout_ms)).await {
        Ok(s) => s,
        Err(_) => return ProbeOutcome::FastReset,
    };

    let req = format!("GET / HTTP/1.1\r\nHost: {host}\r\nUser-Agent: curl/8.0\r\nConnection: close\r\n\r\n");
    if stream.write_all(req.as_bytes()).await.is_err() {
        return ProbeOutcome::FastReset;
    }

    let mut buf = [0u8; 128];
    match tokio::time::timeout(Duration::from_millis(timeout_ms), stream.read(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => ProbeOutcome::ServerResponded,
        Ok(Ok(0)) | Ok(Err(_)) => {
            let elapsed = t.elapsed().as_millis() as u64;
            if elapsed < rtt / 2 { ProbeOutcome::FastReset } else { ProbeOutcome::SlowClose }
        }
        _ => ProbeOutcome::Dropped,
    }
}

fn http_status(line: &str) -> Option<u16> {
    let mut it = line.split_whitespace();
    let proto = it.next()?;
    if !proto.starts_with("HTTP/") {
        return None;
    }
    it.next()?.parse::<u16>().ok()
}

fn looks_like_block_page(resp: &str) -> bool {
    let low = resp.to_lowercase();
    ["доступ ограничен", "доступ заблокирован", "запрещён", "заблокирован",
     "blocklist.rkn", "eais.rkn", "единый реестр", "rkn.gov", "warning.rt.ru"]
        .iter()
        .any(|m| low.contains(m))
}

// ── Test 1: Cross-SNI differential ──────────────────────────────────────────
//
// Measure RTT to a clean RU host, confirm its own SNI answers, then send
// blocked SNIs to the SAME IP. Without DPI the server itself replies with a
// TLS alert (ServerResponded); with DPI a RST/drop arrives faster than RTT
// would allow.
async fn test_differential_sni() -> bool {
    let mut votes_dpi = 0usize;
    let mut votes_total = 0usize;

    for &(ip, clean_sni, port) in CROSS_SNI_TARGETS {
        let rtt = match measure_rtt(ip, port).await {
            Some(r) if r < 1000 => r,
            _ => continue,
        };
        let baseline = probe_tls(ip, port, clean_sni, rtt).await;
        if !matches!(baseline, ProbeOutcome::ServerResponded) {
            continue;
        }
        for &sni in BLOCKED_SNIS {
            let result = probe_tls(ip, port, sni, rtt).await;
            votes_total += 1;
            if is_blocked(&result) {
                votes_dpi += 1;
            }
        }
    }

    votes_total >= 2 && votes_dpi * 2 > votes_total
}

async fn test_differential_http_host() -> bool {
    let http_targets: &[(&str, &str)] = &[("87.240.132.78", "vk.com"), ("77.88.55.242", "ya.ru")];

    let mut votes_dpi = 0usize;
    let mut votes_total = 0usize;

    for &(ip, clean_host) in http_targets {
        let rtt = match measure_rtt(ip, 80).await {
            Some(r) if r < 1000 => r,
            _ => continue,
        };
        let baseline = probe_http(ip, clean_host, rtt).await;
        if !matches!(baseline, ProbeOutcome::ServerResponded) {
            continue;
        }
        for &host in BLOCKED_DOMAINS.iter().take(2) {
            let result = probe_http(ip, host, rtt).await;
            votes_total += 1;
            if is_blocked(&result) {
                votes_dpi += 1;
            }
        }
    }
    votes_total >= 1 && votes_dpi * 2 > votes_total
}

// ── Test: RST injection ─────────────────────────────────────────────────────
// A closed port on a clean IP: a real RST from the server arrives ~RTT
// later; a RST forged by an on-path DPI box arrives faster (it's closer).
async fn test_rst_injection() -> bool {
    let (ip, _, port) = CROSS_SNI_TARGETS[0];
    let rtt = match measure_rtt(ip, port).await {
        Some(r) => r,
        None => return false,
    };

    let Ok(addr) = format!("{ip}:9999").parse::<SocketAddr>() else { return false };
    let t = Instant::now();
    let _ = protected_tcp_connect(addr, Duration::from_millis(rtt * 6)).await;
    let elapsed = t.elapsed().as_millis() as u64;

    elapsed > 0 && elapsed < rtt * 2 / 5
}

// ── Test: TCP fragmentation bypass (GoodbyeDPI-style) ───────────────────────
async fn test_tcp_fragmentation_bypass() -> bool {
    let (ip, _, port) = CROSS_SNI_TARGETS[0];
    let rtt = match measure_rtt(ip, port).await {
        Some(r) => r,
        None => return false,
    };
    let baseline = probe_tls(ip, port, BLOCKED_SNIS[0], rtt).await;
    if !is_blocked(&baseline) {
        return false;
    }
    let Ok(addr) = format!("{ip}:{port}").parse::<SocketAddr>() else { return false };
    let mut stream = match protected_tcp_connect(addr, Duration::from_millis(rtt * 5 + 500)).await {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = stream.set_nodelay(true);
    let hello = build_tls_client_hello(BLOCKED_SNIS[0]);
    if hello.len() < 10 {
        return false;
    }
    if stream.write_all(&hello[..5]).await.is_err() {
        return false;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    if stream.write_all(&hello[5..]).await.is_err() {
        return false;
    }
    let mut buf = [0u8; 128];
    matches!(
        tokio::time::timeout(Duration::from_millis(rtt * 5 + 500), stream.read(&mut buf)).await,
        Ok(Ok(n)) if n > 0
    )
}

// ── Test: random payload on TCP:443 (whitelist DPI) ─────────────────────────
async fn test_random_payload_tcp() -> bool {
    let (ip, _, port) = CROSS_SNI_TARGETS[0];
    let rtt = match measure_rtt(ip, port).await {
        Some(r) => r,
        None => return false,
    };

    let Ok(addr) = format!("{ip}:{port}").parse::<SocketAddr>() else { return false };
    let mut stream = match protected_tcp_connect(addr, Duration::from_millis(rtt * 4)).await {
        Ok(s) => s,
        Err(_) => return false,
    };

    let payload: Vec<u8> = (0u64..64)
        .map(|i| i.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407).wrapping_shr(33) as u8)
        .collect();

    let t = Instant::now();
    if stream.write_all(&payload).await.is_err() {
        return true;
    }

    let mut buf = [0u8; 64];
    match tokio::time::timeout(Duration::from_millis(rtt * 4), stream.read(&mut buf)).await {
        Ok(Ok(0)) | Ok(Err(_)) => (t.elapsed().as_millis() as u64) < rtt,
        _ => false,
    }
}

// ── Test: transparent proxy ─────────────────────────────────────────────────
async fn test_transparent_proxy() -> bool {
    let (ip, clean_host, _) = CROSS_SNI_TARGETS[0];
    let rtt = match measure_rtt(ip, 80).await {
        Some(r) => r,
        None => return false,
    };
    let Ok(addr) = format!("{ip}:80").parse::<SocketAddr>() else { return false };
    let mut stream = match protected_tcp_connect(addr, Duration::from_millis(rtt * 4 + 500)).await {
        Ok(s) => s,
        Err(_) => return false,
    };
    let req = format!("CONNECT {clean_host}:443 HTTP/1.1\r\nHost: {clean_host}:443\r\nProxy-Connection: keep-alive\r\n\r\n");
    if stream.write_all(req.as_bytes()).await.is_err() {
        return false;
    }
    let mut buf = [0u8; 256];
    match tokio::time::timeout(Duration::from_millis(rtt * 4 + 500), stream.read(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => {
            let resp = String::from_utf8_lossy(&buf[..n]);
            let first = resp.lines().next().unwrap_or("");
            matches!(http_status(first), Some(200) | Some(407)) || resp.lines().any(|l| l.to_ascii_lowercase().starts_with("via:"))
        }
        _ => false,
    }
}

async fn test_connect_hijacking() -> bool {
    let (ip, _, _) = CROSS_SNI_TARGETS[0];
    let rtt = match measure_rtt(ip, 80).await {
        Some(r) => r,
        None => return false,
    };
    let Ok(addr) = format!("{ip}:80").parse::<SocketAddr>() else { return false };
    let mut stream = match protected_tcp_connect(addr, Duration::from_millis(rtt * 4 + 500)).await {
        Ok(s) => s,
        Err(_) => return false,
    };
    let req = "CONNECT instagram.com:443 HTTP/1.1\r\nHost: instagram.com:443\r\nProxy-Connection: keep-alive\r\n\r\n";
    if stream.write_all(req.as_bytes()).await.is_err() {
        return false;
    }
    let mut buf = [0u8; 512];
    match tokio::time::timeout(Duration::from_millis(rtt * 4 + 1000), stream.read(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => {
            let resp = String::from_utf8_lossy(&buf[..n]);
            let first = resp.lines().next().unwrap_or("");
            matches!(http_status(first), Some(403) | Some(451)) || looks_like_block_page(&resp)
        }
        _ => false,
    }
}

// ── DNS hijack / injection ──────────────────────────────────────────────────

async fn test_dns_hijacking_detailed() -> (bool, Option<String>) {
    let socket = match protected_udp_socket(false).await {
        Ok(s) => s,
        Err(_) => return (false, None),
    };
    let target: SocketAddr = "8.8.8.8:53".parse().unwrap();
    let query = build_dns_query("google.com");
    if socket.send_to(&query, target).await.is_err() {
        return (false, None);
    }
    let mut buf = [0u8; 512];
    match tokio::time::timeout(Duration::from_millis(2000), socket.recv_from(&mut buf)).await {
        Ok(Ok((_, from))) => {
            let from_ip = from.ip().to_string();
            let hijacked = from_ip != "8.8.8.8";
            let hijacker = if hijacked { Some(from_ip) } else { None };
            (hijacked, hijacker)
        }
        _ => (false, None),
    }
}

async fn dns_query_collect(ip: &str, domain: &str) -> Option<(String, Option<[u8; 4]>)> {
    let socket = protected_udp_socket(false).await.ok()?;
    let target: SocketAddr = format!("{ip}:53").parse().ok()?;
    let id: u16 = 0x33CC;
    let query = build_dns_query_id(id, domain);
    socket.send_to(&query, target).await.ok()?;

    let mut buf = [0u8; 512];
    match tokio::time::timeout(Duration::from_millis(1500), socket.recv_from(&mut buf)).await {
        Ok(Ok((len, from))) if len >= 12 && u16::from_be_bytes([buf[0], buf[1]]) == id && (buf[2] & 0x80) != 0 => {
            Some((from.ip().to_string(), parse_dns_a_record(&buf[..len])))
        }
        _ => None,
    }
}

async fn dns_race_two_answers(resolver_ip: &str, domain: &str) -> Option<String> {
    let socket = protected_udp_socket(false).await.ok()?;
    let target: SocketAddr = format!("{resolver_ip}:53").parse().ok()?;
    let id: u16 = 0x55AA;
    let query = build_dns_query_id(id, domain);
    socket.send_to(&query, target).await.ok()?;

    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut seen: Vec<[u8; 4]> = Vec::new();
    let mut buf = [0u8; 512];
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, _))) if len >= 12 && u16::from_be_bytes([buf[0], buf[1]]) == id => {
                if let Some(a) = parse_dns_a_record(&buf[..len]) {
                    if !seen.contains(&a) {
                        seen.push(a);
                    }
                }
            }
            _ => break,
        }
    }

    if seen.len() >= 2 {
        let fmt = |a: &[u8; 4]| format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3]);
        Some(format!(
            "две разные A-записи в гонке ({}, {}) — поддельный ответ обогнал настоящий",
            fmt(&seen[0]),
            fmt(&seen[1])
        ))
    } else {
        None
    }
}

/// Method A: query a BLOCKED domain against a host that is NOT a DNS server
/// (the VK web IP on :53). No legitimate answer can physically arrive — any
/// reply is a middlebox forging one. Method B: race two answers from a real
/// resolver — two different A records under the same query ID means an
/// injected reply beat the genuine one.
async fn test_dns_injection() -> (bool, Option<String>) {
    let dead_host = CROSS_SNI_TARGETS[0].0;
    for domain in BLOCKED_DOMAINS.iter().take(2) {
        if let Some((from_ip, a_rec)) = dns_query_collect(dead_host, domain).await {
            let a = a_rec.map(|i| format!("{}.{}.{}.{}", i[0], i[1], i[2], i[3])).unwrap_or_else(|| "без A-записи".into());
            return (true, Some(format!("{domain}: поддельный ответ на :53 от {from_ip} (вернул {a}); хост не DNS-сервер")));
        }
    }
    for domain in BLOCKED_DOMAINS.iter().take(2) {
        if let Some(detail) = dns_race_two_answers("8.8.8.8", domain).await {
            return (true, Some(format!("{domain}: {detail}")));
        }
    }
    (false, None)
}

// ── UDP throttling ───────────────────────────────────────────────────────────
async fn test_udp_throttle() -> bool {
    let socket = match protected_udp_socket(false).await {
        Ok(s) => s,
        Err(_) => return false,
    };
    let target: SocketAddr = "8.8.8.8:53".parse().unwrap();
    let query = build_dns_query("vk.com");
    let mut latencies: Vec<u64> = Vec::with_capacity(10);

    for _ in 0..10 {
        let t = Instant::now();
        let _ = socket.send_to(&query, target).await;
        let mut buf = [0u8; 512];
        if tokio::time::timeout(Duration::from_millis(600), socket.recv_from(&mut buf)).await.is_ok() {
            latencies.push(t.elapsed().as_millis() as u64);
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
    }

    if latencies.len() < 5 {
        return false;
    }
    let avg = latencies.iter().sum::<u64>() / latencies.len() as u64;
    let max = *latencies.iter().max().unwrap_or(&0);
    let variance = latencies.iter().map(|&x| { let d = x as i64 - avg as i64; (d * d) as u64 }).sum::<u64>() / latencies.len() as u64;
    let std_dev = (variance as f64).sqrt() as u64;

    (max > 5 * avg && avg > 10) || (std_dev > 2 * avg && avg > 15)
}

// ── DNS server reachability/interception table ──────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct DnsServerStatus {
    pub server: String,
    pub reachable: bool,
    pub rtt_ms: u64,
    pub actual_responder: Option<String>,
    pub intercepted: bool,
}

async fn test_system_dns_servers() -> Vec<DnsServerStatus> {
    let servers: &[(&str, &str)] = &[
        ("8.8.8.8:53", "8.8.8.8"),
        ("1.1.1.1:53", "1.1.1.1"),
        ("9.9.9.9:53", "9.9.9.9"),
        ("77.88.8.8:53", "77.88.8.8"),
        ("94.140.14.14:53", "94.140.14.14"),
    ];

    let mut results = Vec::new();
    let test_domain = "google.com";

    for (server_addr, expected_ip) in servers {
        let Ok(socket) = protected_udp_socket(false).await else { continue };
        let Ok(target) = server_addr.parse::<SocketAddr>() else { continue };
        let query = build_dns_query(test_domain);
        let t = Instant::now();

        if socket.send_to(&query, target).await.is_err() {
            results.push(DnsServerStatus { server: server_addr.to_string(), reachable: false, rtt_ms: 0, actual_responder: None, intercepted: false });
            continue;
        }

        let mut buf = [0u8; 512];
        match tokio::time::timeout(Duration::from_millis(2000), socket.recv_from(&mut buf)).await {
            Ok(Ok((_, from))) => {
                let rtt_ms = t.elapsed().as_millis() as u64;
                let from_ip = from.ip().to_string();
                let intercepted = &from_ip != expected_ip;
                results.push(DnsServerStatus {
                    server: server_addr.to_string(),
                    reachable: true,
                    rtt_ms,
                    actual_responder: if intercepted { Some(from_ip) } else { None },
                    intercepted,
                });
            }
            _ => results.push(DnsServerStatus { server: server_addr.to_string(), reachable: false, rtt_ms: 0, actual_responder: None, intercepted: false }),
        }
    }

    results
}

fn parse_dns_a_record(data: &[u8]) -> Option<[u8; 4]> {
    if data.len() < 12 {
        return None;
    }
    let ancount = u16::from_be_bytes([data[6], data[7]]);
    if ancount == 0 {
        return None;
    }
    let mut pos = 12;
    while pos < data.len() {
        let len = data[pos] as usize;
        if len == 0 { pos += 1; break; }
        if len >= 0xC0 { pos += 2; break; }
        pos += 1 + len;
    }
    if pos + 4 > data.len() {
        return None;
    }
    pos += 4;
    while pos + 10 < data.len() {
        if data[pos] >= 0xC0 {
            pos += 2;
        } else {
            while pos < data.len() && data[pos] != 0 { pos += 1; }
            pos += 1;
        }
        if pos + 10 > data.len() { break; }
        let rtype = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let rdlen = u16::from_be_bytes([data[pos + 8], data[pos + 9]]) as usize;
        pos += 10;
        if rtype == 1 && rdlen == 4 && pos + 4 <= data.len() {
            return Some([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        }
        pos += rdlen;
    }
    None
}

fn build_dns_query(name: &str) -> Vec<u8> {
    build_dns_query_id(0xABCD, name)
}

fn build_dns_query_id(id: u16, name: &str) -> Vec<u8> {
    let mut msg = Vec::with_capacity(name.len() + 18);
    msg.extend_from_slice(&id.to_be_bytes());
    msg.extend_from_slice(&[0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    for part in name.split('.') {
        msg.push(part.len() as u8);
        msg.extend_from_slice(part.as_bytes());
    }
    msg.push(0x00);
    msg.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);
    msg
}

// ── TLS ClientHello builder ───────────────────────────────────────────────────

fn build_tls_client_hello(sni: &str) -> Vec<u8> {
    let sni_bytes = sni.as_bytes();
    let sni_len = sni_bytes.len();
    let mut h = vec![];
    h.push(0x16); h.push(0x03); h.push(0x01);
    let rec_len_pos = h.len(); h.push(0x00); h.push(0x00);
    let hs_start = h.len();
    h.push(0x01);
    let hs_len_pos = h.len(); h.push(0x00); h.push(0x00); h.push(0x00);
    let ch_start = h.len();
    h.push(0x03); h.push(0x03);
    h.extend_from_slice(&[0x5B; 32]);
    h.push(0x00);
    h.extend_from_slice(&[0x00, 0x04, 0x13, 0x01, 0xc0, 0x2b]);
    h.push(0x01); h.push(0x00);
    let ext_len_pos = h.len(); h.push(0x00); h.push(0x00);
    h.push(0x00); h.push(0x00);
    let ext_data_len = (2 + 1 + 2 + sni_len) as u16;
    h.push((ext_data_len >> 8) as u8); h.push((ext_data_len & 0xff) as u8);
    let name_list_len = (1 + 2 + sni_len) as u16;
    h.push((name_list_len >> 8) as u8); h.push((name_list_len & 0xff) as u8);
    h.push(0x00);
    h.push((sni_len >> 8) as u8); h.push((sni_len & 0xff) as u8);
    h.extend_from_slice(sni_bytes);
    h.extend_from_slice(&[0x00, 0x2b, 0x00, 0x03, 0x02, 0x03, 0x04]);
    h.extend_from_slice(&[0x00, 0x0a, 0x00, 0x06, 0x00, 0x04, 0x00, 0x1d, 0x00, 0x17]);
    let ext_total = h.len() - ext_len_pos - 2;
    h[ext_len_pos] = (ext_total >> 8) as u8; h[ext_len_pos + 1] = (ext_total & 0xff) as u8;
    let ch_len = h.len() - ch_start;
    h[hs_len_pos + 1] = (ch_len >> 8) as u8; h[hs_len_pos + 2] = (ch_len & 0xff) as u8;
    let rec_len = h.len() - hs_start;
    h[rec_len_pos] = (rec_len >> 8) as u8; h[rec_len_pos + 1] = (rec_len & 0xff) as u8;
    h
}

// ── Orchestrator ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct DpiBatteryReport {
    pub rst_injection_detected: bool,
    pub http_host_blocked: bool,
    pub sni_blocked: bool,
    pub vulnerable_to_fragmentation: bool,
    pub random_payload_blocked: bool,
    pub udp_throttled: bool,
    pub dns_hijacked: bool,
    pub dns_hijacker_ip: Option<String>,
    pub dns_injected: bool,
    pub dns_injection_msg: Option<String>,
    pub transparent_proxy_detected: bool,
    pub connect_hijacked: bool,
    pub dns_servers: Vec<DnsServerStatus>,
    pub dpi_score: f32,
}

/// Runs the full battery against fixed, well-known public targets (not the
/// user's ostp server) to characterize what the current network path filters
/// in general. Takes ~10s. Every socket is protected against the VPN tunnel
/// (see module docs), so this is safe to run while connected.
pub async fn run_dpi_battery() -> DpiBatteryReport {
    let (
        sni_blocked,
        http_host_blocked,
        random_payload_blocked,
        udp_throttled,
        (dns_hijacked, dns_hijacker_ip),
        (dns_injected, dns_injection_msg),
        transparent_proxy,
        connect_hijacked,
        dns_servers,
    ) = tokio::join!(
        test_differential_sni(),
        test_differential_http_host(),
        test_random_payload_tcp(),
        test_udp_throttle(),
        test_dns_hijacking_detailed(),
        test_dns_injection(),
        test_transparent_proxy(),
        test_connect_hijacking(),
        test_system_dns_servers(),
    );

    let rst_injection = test_rst_injection().await;

    let vulnerable_to_fragmentation = if sni_blocked { test_tcp_fragmentation_bypass().await } else { false };

    let mut score: f32 = 0.0;
    if sni_blocked && http_host_blocked { score += 0.90; }
    else if sni_blocked { score += 0.75; }
    else if http_host_blocked { score += 0.65; }

    if dns_hijacked && dns_injected { score += 0.30; }
    else if dns_hijacked || dns_injected { score += 0.20; }

    if connect_hijacked { score += 0.20; }
    if transparent_proxy { score += 0.10; }

    if score < 0.4 {
        if rst_injection { score += 0.35; }
        if random_payload_blocked { score += 0.15; }
    }
    if udp_throttled { score += 0.10; }

    DpiBatteryReport {
        rst_injection_detected: rst_injection,
        http_host_blocked,
        sni_blocked,
        vulnerable_to_fragmentation,
        random_payload_blocked,
        udp_throttled,
        dns_hijacked,
        dns_hijacker_ip,
        dns_injected,
        dns_injection_msg,
        transparent_proxy_detected: transparent_proxy,
        connect_hijacked,
        dns_servers,
        dpi_score: score.min(1.0),
    }
}
