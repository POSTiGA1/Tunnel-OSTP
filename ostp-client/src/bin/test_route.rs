//! Prints which process the system reports for this program's own sockets:
//! a quick check of per-process exclusions on a new machine.
use ostp_client::tunnel::process_lookup::{process_owning, Proto};

fn main() {
    let udp = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
    let addr = udp.local_addr().unwrap();
    println!("UDP {addr}: {:?}", process_owning(Proto::Udp, addr));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let addr = client.local_addr().unwrap();
    println!("TCP {addr}: {:?}", process_owning(Proto::Tcp, addr));
}
