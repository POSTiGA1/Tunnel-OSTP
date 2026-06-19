use anyhow::{anyhow, Result};
use tokio::net::TcpStream;

#[cfg(target_os = "windows")]
pub fn bind_socket_to_interface(socket: &tokio::net::TcpSocket, is_ipv6: bool, if_index: u32) -> std::io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use winapi::shared::ws2def::{IPPROTO_IP, IPPROTO_IPV6};
    
    // These constants are defined as 31 in the Windows SDK.
    const IP_UNICAST_IF: i32 = 31;
    const IPV6_UNICAST_IF: i32 = 31;

    let fd = socket.as_raw_socket() as usize;
    let idx_net = if_index.to_be();

    let (level, optname) = if is_ipv6 {
        (IPPROTO_IPV6 as i32, IPV6_UNICAST_IF)
    } else {
        (IPPROTO_IP as i32, IP_UNICAST_IF)
    };

    let ret = unsafe {
        winapi::um::winsock2::setsockopt(
            fd,
            level as i32,
            optname as i32,
            &idx_net as *const _ as *const i8,
            std::mem::size_of_val(&idx_net) as i32,
        )
    };

    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn bind_socket_to_interface(socket: &tokio::net::TcpSocket, _is_ipv6: bool, if_name: &str) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    let fd = socket.as_raw_fd();
    let name_bytes = if_name.as_bytes();
    let ret = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_BINDTODEVICE,
            name_bytes.as_ptr() as *const libc::c_void,
            name_bytes.len() as libc::socklen_t,
        )
    };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn bind_socket_to_interface(socket: &tokio::net::TcpSocket, _is_ipv6: bool, if_index: u32) -> std::io::Result<()> {
    // macOS uses IP_BOUND_IF for IPv4 and IPV6_BOUND_IF for IPv6, similar to Windows
    use std::os::unix::io::AsRawFd;
    let fd = socket.as_raw_fd();
    
    // We can implement this later, for now just a stub so compilation works
    tracing::debug!("macOS socket binding not yet fully implemented for interface {}", if_index);
    Ok(())
}

pub async fn dial_tcp(target_host: &str, target_port: u16, _phys_if_idx: Option<u32>) -> Result<TcpStream> {
    let addrs = tokio::net::lookup_host((target_host, target_port)).await?.collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(anyhow!("Could not resolve target host: {}", target_host));
    }

    let target_addr = addrs[0];
    let socket = match target_addr {
        std::net::SocketAddr::V4(_) => tokio::net::TcpSocket::new_v4()?,
        std::net::SocketAddr::V6(_) => tokio::net::TcpSocket::new_v6()?,
    };

    #[cfg(target_os = "windows")]
    if let Some(idx) = _phys_if_idx {
        if let Err(e) = bind_socket_to_interface(&socket, target_addr.is_ipv6(), idx) {
            tracing::warn!("DIRECT: Failed to bind to physical interface {}: {}", idx, e);
        }
    }

    let stream = tokio::time::timeout(std::time::Duration::from_secs(10), socket.connect(target_addr)).await??;
    Ok(stream)
}

pub async fn handle_udp(
    _client_src: std::net::SocketAddr,
    _target_dst: std::net::SocketAddr,
    _payload: bytes::Bytes,
    _phys_if_idx: Option<u32>,
) -> Result<()> {
    Err(anyhow!("Direct UDP is not yet fully implemented"))
}
