fn main() {
    let route = ostp_tun::windows::windows_route::sys::get_default_ipv4_route();
    println!("Default IPv4 route: {:?}", route);
}
