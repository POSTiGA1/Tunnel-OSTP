use std::sync::Arc;
use tokio::net::UdpSocket;
use bytes::Bytes;

#[derive(Clone)]
pub enum Transport {
    Udp(Arc<UdpSocket>),
    Uot {
        tx: tokio::sync::mpsc::Sender<Bytes>,
        rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Bytes>>>,
    },
    Dnstt {
        tx: tokio::sync::mpsc::Sender<Bytes>,
        rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Bytes>>>,
        _guard: Arc<tokio::sync::Mutex<ostp_core::dnstt::DnsttProcess>>,
    }
}

impl Transport {
    pub async fn send(&self, frame: &Bytes) -> std::io::Result<usize> {
        match self {
            Self::Udp(sock) => sock.send(frame).await,
            Self::Uot { tx, .. } | Self::Dnstt { tx, .. } => {
                tx.send(frame.clone()).await.map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "channel closed"))?;
                Ok(frame.len())
            }
        }
    }

    pub async fn send_to(&self, frame: &Bytes, target: std::net::SocketAddr) -> std::io::Result<usize> {
        match self {
            Self::Udp(sock) => sock.send_to(frame, target).await,
            Self::Uot { .. } | Self::Dnstt { .. } => self.send(frame).await,
        }
    }

    pub async fn recv(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Udp(sock) => sock.recv(buf).await,
            Self::Uot { rx, .. } | Self::Dnstt { rx, .. } => {
                let mut rx = rx.lock().await;
                if let Some(frame) = rx.recv().await {
                    let len = frame.len().min(buf.len());
                    buf[..len].copy_from_slice(&frame[..len]);
                    Ok(len)
                } else {
                    Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "channel closed"))
                }
            }
        }
    }

    pub async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, std::net::SocketAddr)> {
        match self {
            Self::Udp(sock) => sock.recv_from(buf).await,
            Self::Uot { .. } | Self::Dnstt { .. } => {
                let n = self.recv(buf).await?;
                Ok((n, "127.0.0.1:0".parse().unwrap()))
            }
        }
    }

    pub fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        match self {
            Self::Udp(sock) => sock.local_addr(),
            Self::Uot { .. } | Self::Dnstt { .. } => Ok("0.0.0.0:0".parse().unwrap()),
        }
    }
}
