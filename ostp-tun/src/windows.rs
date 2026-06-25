use crate::{OstpTunInterface, OstpTunOptions};
use anyhow::{anyhow, Result};
use tun::AbstractDeviceExt;

pub mod windows_route {
    include!("windows_route.rs");
}

#[path = "sys_win_api.rs"]
mod sys_win_api;

struct WindowsRouteGuard {
    bypass_routes: Vec<(std::net::Ipv4Addr, std::net::Ipv4Addr, u32)>,
    kill_switch: bool,
}

impl Drop for WindowsRouteGuard {
    fn drop(&mut self) {
        windows_route::sys::remove_bypass_routes(&self.bypass_routes);
        tracing::info!("Removed {} bypass routes.", self.bypass_routes.len());

        let _ = sys_win_api::remove_firewall_rules();

        if self.kill_switch {
            let _ = sys_win_api::set_kill_switch_route(false);
        }
    }
}

pub async fn create(opts: OstpTunOptions) -> Result<OstpTunInterface> {
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let (phys_gw, phys_if) = windows_route::sys::get_default_ipv4_route()
        .ok_or_else(|| anyhow!("Cannot find physical default IPv4 route"))?;

    let mut bypass_v4: Vec<std::net::Ipv4Addr> = Vec::new();
    if let std::net::IpAddr::V4(v4) = opts.server_ip {
        bypass_v4.push(v4);
    }
    for ip in opts.bypass_ips {
        if let std::net::IpAddr::V4(v4) = ip {
            bypass_v4.push(v4);
        }
    }

    let bypass_routes = windows_route::sys::add_bypass_routes(&bypass_v4, phys_gw, phys_if, 1);
    tracing::info!("Added {} bypass routes via {} (if_index={})", bypass_routes.len(), phys_gw, phys_if);

    // The bypass route for the OSTP server itself is mandatory: the TUN default
    // route installed below captures ALL traffic, so without a /32 carve-out the
    // client's own connection to the server loops back into the tunnel — every
    // handshake times out and there is no connectivity. Treat a missing server
    // bypass as fatal so the connect flow aborts cleanly instead of coming up in a
    // fake "connected" state with no internet.
    if let std::net::IpAddr::V4(server_v4) = opts.server_ip {
        if !server_v4.is_loopback()
            && !server_v4.is_unspecified()
            && !bypass_routes.iter().any(|(ip, _, _)| *ip == server_v4)
        {
            windows_route::sys::remove_bypass_routes(&bypass_routes);
            return Err(anyhow!(
                "Failed to install bypass route for OSTP server {server_v4}. Without it the \
                 tunnel would capture its own server connection (routing loop, no internet). \
                 Aborting tunnel startup."
            ));
        }
    }

    // No need to call powershell to clean up adapters, WinTun handles it well
    // if we don't try to use friendly-name matching.

    let mut tun_cfg = tun::Configuration::default();
    tun_cfg
        .tun_name("ostp_tun")
        .address((10, 1, 0, 2))
        .netmask((255, 255, 255, 0))
        // `.destination()` makes the `tun` crate install the default route via
        // the adapter's LUID (available immediately after creation). This is
        // the RELIABLE default route that captures traffic — the manual
        // add_ipv4_route below depends on a friendly-name interface-index
        // lookup that races on a freshly created adapter. Keep both: the LUID
        // route guarantees connectivity even when the index lookup is slow.
        // The retry loop around tun::create absorbs the transient
        // ERROR_INVALID_PARAMETER (os error 87) this call can hit during the
        // post-creation registration window.
        .destination((10, 1, 0, 1))
        .mtu(opts.mtu)
        .up();

    // The IpHelper calls the `tun` crate performs right after Adapter::create
    // (set address / mtu) can transiently fail with ERROR_INVALID_PARAMETER
    // (os error 87) when the freshly created interface is not yet registered
    // in the IP stack. Retry a few times; on retry the crate reuses the
    // existing adapter via Adapter::open.
    let dev = {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match tun::create(&tun_cfg) {
                Ok(d) => break d,
                Err(e) if attempt < 5 => {
                    tracing::warn!(
                        "TUN device creation attempt {}/5 failed: {} — retrying in 300ms",
                        attempt, e
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
                Err(e) => return Err(anyhow!("Failed to create TUN device after {} attempts: {}", attempt, e)),
            }
        }
    };
    let luid = dev.tun_luid();
    let dev = tun::AsyncDevice::new(dev).map_err(|e| anyhow!("TUN device async failed: {}", e))?;
    tracing::info!("TUN device 'ostp_tun' created.");
    // We rely entirely on the LUID-based route established by the `tun` crate's `.destination()`.
    // It is instant and reliable. The fallback polling loop has been removed for instant startup.

    let current_exe = std::env::current_exe()?.to_string_lossy().into_owned();
    let exe1 = current_exe.clone();
    let _ = tokio::task::spawn_blocking(move || {
        if let Err(e) = sys_win_api::add_firewall_rules(&exe1) {
            tracing::warn!("Failed to add firewall rules via WinAPI: {}", e);
        }
    });

    if let Some(ref dns) = opts.dns_server {
        if !dns.is_empty() {
            let dns_clone = dns.clone();
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = sys_win_api::set_dns_servers(luid, &dns_clone) {
                    tracing::warn!("Failed to set DNS via WinAPI: {}", e);
                }
            });
        }
    }

    if opts.kill_switch {
        tracing::info!("Kill Switch enabled: Adding metric 10 blackhole route to prevent leakage");
        let _ = sys_win_api::set_kill_switch_route(true);
    }

    Ok(OstpTunInterface {
        device: dev,
        guard: Box::new(WindowsRouteGuard {
            bypass_routes,
            kill_switch: opts.kill_switch,
        }),
    })
}
