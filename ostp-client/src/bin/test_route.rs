fn main() {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    println!("Bound UDP to port {}", port);
    
    if let Some(name) = ostp_client::tunnel::process_lookup::get_process_name_from_port_udp(port) {
        println!("Found process for UDP port {}: {}", port, name);
    } else {
        println!("Process not found for UDP port {}", port);
    }

    let tcp_socket = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    let tcp_port = tcp_socket.local_addr().unwrap().port();
    println!("Bound TCP to port {}", tcp_port);

    if let Some(name) = ostp_client::tunnel::process_lookup::get_process_name_from_port(tcp_port) {
        println!("Found process for TCP port {}: {}", tcp_port, name);
    } else {
        println!("Process not found for TCP port {}", tcp_port);
    }
}
