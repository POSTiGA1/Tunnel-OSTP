
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use rand::Rng;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use bytes::Bytes;

use crate::debug_preview::describe_foreign_bytes;

/// UoT/TCP connection parameters shared by the live bridge and the prober.
#[derive(Clone)]
pub struct UotOptions {
    pub tcp_fragmentation: bool,
    pub frag_chunk: usize,
    pub frag_sleep: u64,
    pub junk_pc: [usize; 2],
    pub junk_ps: [usize; 2],
    pub access_key: Bytes,
    /// IP_TTL / hop-limit override for this connection's outbound packets.
    /// `None` leaves the OS default. Used by the prober's TTL/TSPU scan to
    /// find the hop distance at which an operator's middlebox starts
    /// answering in place of the real server.
    pub ttl: Option<u32>,
    pub connect_timeout: Duration,
}

/// Opens a UoT/TCP transport: junk packets, then an optional fragmented
/// first frame, matching the on-wire shape a real ostp server expects.
///
/// The returned receiver carries a human-readable note each time the
/// connection ends or errors with bytes left over that never formed a
/// complete ostp frame — the signature of an operator/DPI transparent proxy
/// answering in place of the real server (see `describe_foreign_bytes`).
/// Callers that don't need this (or that gate it behind their own debug
/// flag) can simply drop the receiver.
pub async fn connect_uot(
    target_ip: IpAddr,
    port: u16,
    opts: UotOptions,
) -> anyhow::Result<(Transport, mpsc::UnboundedReceiver<String>)> {
    let stream = tokio::time::timeout(
        opts.connect_timeout,
        tokio::net::TcpStream::connect((target_ip, port)),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "TCP connect to {target_ip}:{port} timed out after {:?}",
            opts.connect_timeout
        )
    })??;
    let _ = stream.set_nodelay(true);
    if let Some(ttl) = opts.ttl {
        let _ = stream.set_ttl(ttl);
    }
    let (mut read_half, mut write_half) = stream.into_split();

    let tcp_fragmentation = opts.tcp_fragmentation;
    let frag_chunk = opts.frag_chunk.max(1);
    let frag_sleep = opts.frag_sleep;
    let [junk_pc_min, junk_pc_max] = opts.junk_pc;
    let [junk_ps_min, junk_ps_max] = opts.junk_ps;
    // Time-rotating per-key junk marker — NOT a global constant and NOT
    // even a static per-user value: it changes every window, so junk
    // carries no fixed DPI signature on the wire. All frames in this
    // burst are sent within milliseconds, so one window applies to all.
    let junk_marker = ostp_core::crypto::derive_junk_marker(
        &opts.access_key,
        ostp_core::crypto::current_junk_window(),
    );

    {
        use tokio::io::AsyncWriteExt;
        // Build all junk frames up front so ThreadRng isn't held across an
        // await point (keeps this future Send).
        let junk_frames: Vec<Vec<u8>> = {
            let mut rng = rand::thread_rng();
            let min_c = junk_pc_min;
            let max_c = junk_pc_max.max(min_c);
            let num_junk = rng.gen_range(min_c..=max_c);
            (0..num_junk)
                .map(|_| {
                    let min_s = junk_ps_min.max(1);
                    let max_s = junk_ps_max.max(min_s);
                    let junk_len = rng.gen_range(min_s..=max_s);
                    let mut frame = Vec::with_capacity(2 + junk_len);
                    frame.extend_from_slice(&(junk_len as u16).to_be_bytes());
                    let start = frame.len();
                    frame.resize(start + junk_len, 0);
                    rng.fill(&mut frame[start..]);
                    // Stamp this key's derived junk marker so the server drops it silently.
                    if junk_len >= 4 {
                        frame[start..start + 4].copy_from_slice(&junk_marker);
                    }
                    frame
                })
                .collect()
        };
        for frame in junk_frames {
            if write_half.write_all(&frame).await.is_err() { break; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    let (tx_out, mut rx_out) = mpsc::channel::<Bytes>(1024);
    let (tx_in, rx_in) = mpsc::channel::<Bytes>(1024);
    let (foreign_tx, foreign_rx) = mpsc::unbounded_channel::<String>();

    // Writer: length-prefix each frame. With tcp_fragmentation on, split
    // the FIRST real frame (the handshake — junk above was written
    // directly, so it doesn't count) into tiny TCP segments with short
    // gaps so DPI can't reassemble/classify the handshake from one read.
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let mut first_packet = true;
        while let Some(data) = rx_out.recv().await {
            let len_buf = (data.len() as u16).to_be_bytes();
            if first_packet && tcp_fragmentation {
                first_packet = false;
                if write_half.write_all(&len_buf[0..1]).await.is_err() { break; }
                tokio::time::sleep(Duration::from_millis(5)).await;
                if write_half.write_all(&len_buf[1..2]).await.is_err() { break; }
                tokio::time::sleep(Duration::from_millis(5)).await;
                let mut broke = false;
                for chunk in data.chunks(frag_chunk) {
                    if write_half.write_all(chunk).await.is_err() { broke = true; break; }
                    tokio::time::sleep(Duration::from_millis(frag_sleep)).await;
                }
                if broke { break; }
            } else {
                if write_half.write_all(&len_buf).await.is_err() { break; }
                if write_half.write_all(&data).await.is_err() { break; }
            }
        }
    });

    // Reader: reads whatever is available and only pulls a frame out once the
    // full [len:2][payload] is in hand, instead of read_exact-ing the length
    // prefix and then the body as two separate blocking reads. That
    // distinction matters on mobile networks: some operators' DPI/transparent
    // proxy answers the TCP connection itself with a short block/redirect
    // page (a few hundred bytes) instead of relaying to the real ostp server.
    // Reading those bytes as a bogus length prefix would block on read_exact
    // for a body that never arrives and just time out with nothing to show
    // for it. This version notices the stream ending mid-frame and reports
    // the leftover bytes on `foreign_tx` — which by construction never
    // includes a successfully-parsed (and therefore genuine) ostp frame.
    let tx_in_clone = tx_in.clone();
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut acc: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            if acc.len() >= 2 {
                let len = u16::from_be_bytes([acc[0], acc[1]]) as usize;
                if acc.len() >= 2 + len {
                    let data = acc[2..2 + len].to_vec();
                    acc.drain(0..2 + len);
                    if tx_in_clone.send(Bytes::from(data)).await.is_err() { break; }
                    continue;
                }
            }
            match read_half.read(&mut chunk).await {
                Ok(0) => {
                    if !acc.is_empty() {
                        let _ = foreign_tx.send(format!(
                            "connection closed with {} unparsed byte(s) left over \
                             (does not look like an ostp frame — possible operator/DPI \
                             interference): {}",
                            acc.len(),
                            describe_foreign_bytes(&acc)
                        ));
                    }
                    break;
                }
                Ok(n) => acc.extend_from_slice(&chunk[..n]),
                Err(e) => {
                    if !acc.is_empty() {
                        let _ = foreign_tx.send(format!(
                            "read error after {} unparsed byte(s) \
                             (does not look like an ostp frame — possible operator/DPI \
                             interference): {} ({})",
                            acc.len(),
                            describe_foreign_bytes(&acc),
                            e
                        ));
                    }
                    break;
                }
            }
        }
    });

    Ok((
        Transport::Uot { tx: tx_out, rx: Arc::new(Mutex::new(rx_in)) },
        foreign_rx,
    ))
}

#[derive(Clone)]
pub enum Transport {
    Udp(Arc<UdpSocket>),
    Uot {
        tx: tokio::sync::mpsc::Sender<Bytes>,
        rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Bytes>>>,
    }
}

impl Transport {
    pub async fn send(&self, frame: &Bytes) -> std::io::Result<usize> {
        match self {
            Self::Udp(sock) => sock.send(frame).await,
            Self::Uot { tx, .. } => {
                tx.send(frame.clone()).await.map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "uot closed"))?;
                Ok(frame.len())
            }
        }
    }

    pub async fn send_to(&self, frame: &Bytes, target: std::net::SocketAddr) -> std::io::Result<usize> {
        match self {
            Self::Udp(sock) => sock.send_to(frame, target).await,
            Self::Uot { .. } => self.send(frame).await,
        }
    }

    pub async fn recv(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Udp(sock) => sock.recv(buf).await,
            Self::Uot { rx, .. } => {
                let mut rx = rx.lock().await;
                match rx.recv().await {
                    Some(bytes) => {
                        let len = bytes.len().min(buf.len());
                        buf[..len].copy_from_slice(&bytes[..len]);
                        Ok(len)
                    }
                    None => Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "uot closed")),
                }
            }
        }
    }

    pub fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        match self {
            Self::Udp(sock) => sock.local_addr(),
            Self::Uot { .. } => Ok("0.0.0.0:0".parse().unwrap()),
        }
    }
}
