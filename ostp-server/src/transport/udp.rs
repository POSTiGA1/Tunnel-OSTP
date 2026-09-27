//! The server's UDP sockets, one per `listen` address, and the choice of which
//! one answers a given client.
//!
//! Replies must leave through a socket that can reach the client, and should
//! come from the address the client sent to. Sending everything through the
//! first socket broke UDP whenever that one was bound to loopback (a config
//! with `127.0.0.1:50000` listed first, for a web server in front): the kernel
//! refuses to send from 127.0.0.1 to a public address (EINVAL), so the server
//! accepted every handshake and could not answer any of them.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::net::UdpSocket;

/// Bound on remembered client addresses. Past it the table is cleared and
/// rebuilt from the next packets; until then replies use the fallback choice,
/// which is right for every single-address-per-family setup.
const MAX_REMEMBERED_PEERS: usize = 65_536;

pub struct UdpSockets {
    sockets: Vec<Arc<UdpSocket>>,
    /// Socket index that last received from each client address.
    reply_via: Mutex<HashMap<SocketAddr, usize>>,
}

impl UdpSockets {
    pub fn new(sockets: Vec<Arc<UdpSocket>>) -> Self {
        Self { sockets, reply_via: Mutex::new(HashMap::new()) }
    }

    pub fn sockets(&self) -> &[Arc<UdpSocket>] {
        &self.sockets
    }

    /// Records that `peer` reached us on socket `index`, so replies to it
    /// leave from the same local address.
    pub fn received(&self, index: usize, peer: SocketAddr) {
        let mut map = self.reply_via.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() >= MAX_REMEMBERED_PEERS && !map.contains_key(&peer) {
            map.clear();
        }
        map.insert(peer, index);
    }

    pub async fn send_to(&self, buf: &[u8], peer: SocketAddr) -> std::io::Result<usize> {
        let index = self.socket_for(peer).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::AddrNotAvailable,
                format!("no listen address can send to {peer}"),
            )
        })?;
        self.sockets[index].send_to(buf, peer).await
    }

    fn socket_for(&self, peer: SocketAddr) -> Option<usize> {
        let remembered = self.reply_via.lock().unwrap_or_else(|e| e.into_inner()).get(&peer).copied();
        remembered.or_else(|| {
            let locals: Vec<Option<SocketAddr>> = self.sockets.iter().map(|s| s.local_addr().ok()).collect();
            pick_socket(&locals, peer)
        })
    }
}

/// The socket a reply to an unknown `peer` goes out of: same address family,
/// and loopback only for a loopback peer. A wildcard bind reaches anything.
fn pick_socket(locals: &[Option<SocketAddr>], peer: SocketAddr) -> Option<usize> {
    let peer_ip = peer.ip().to_canonical();
    let can_reach = |local: &SocketAddr| {
        let local_ip = local.ip();
        local.is_ipv4() == peer_ip.is_ipv4()
            && (local_ip.is_unspecified() || local_ip.is_loopback() == peer_ip.is_loopback())
    };
    locals.iter().position(|l| l.as_ref().is_some_and(can_reach))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addrs(list: &[&str]) -> Vec<Option<SocketAddr>> {
        list.iter().map(|a| Some(a.parse().unwrap())).collect()
    }

    fn peer(a: &str) -> SocketAddr {
        a.parse().unwrap()
    }

    #[test]
    fn a_public_client_is_not_answered_from_loopback() {
        let locals = addrs(&["127.0.0.1:50000", "31.76.224.166:50000"]);
        assert_eq!(pick_socket(&locals, peer("88.151.94.82:54907")), Some(1));
        assert_eq!(pick_socket(&locals, peer("127.0.0.1:40000")), Some(0));
    }

    #[test]
    fn families_are_matched_and_wildcards_reach_anything() {
        let locals = addrs(&["127.0.0.1:50000", "[::]:50000", "0.0.0.0:50000"]);
        assert_eq!(pick_socket(&locals, peer("[2001:db8::1]:1000")), Some(1));
        assert_eq!(pick_socket(&locals, peer("198.51.100.7:1000")), Some(2));
        assert_eq!(pick_socket(&addrs(&["127.0.0.1:50000"]), peer("198.51.100.7:1000")), None);
    }

    #[tokio::test]
    async fn replies_leave_from_the_socket_the_client_used() {
        let a = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let b = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let (a_addr, b_addr) = (a.local_addr().unwrap(), b.local_addr().unwrap());
        let sockets = UdpSockets::new(vec![a, b]);

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_addr = client.local_addr().unwrap();
        sockets.received(1, client_addr);
        sockets.send_to(b"hi", client_addr).await.unwrap();

        let mut buf = [0u8; 8];
        let (n, from) = client.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hi");
        assert_eq!(from, b_addr);
        assert_ne!(from, a_addr);
    }
}
