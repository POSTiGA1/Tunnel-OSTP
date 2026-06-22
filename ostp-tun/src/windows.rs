use crate::{OstpTunInterface, OstpTunOptions};
use anyhow::{anyhow, Result};
use std::process::Command;
use std::os::windows::process::CommandExt;

pub mod windows_route {
    include!("windows_route.rs");
}

struct WindowsRouteGuard {
    bypass_routes: Vec<(std::net::Ipv4Addr, std::net::Ipv4Addr, u32)>,
    kill_switch: bool,
}

impl Drop for WindowsRouteGuard {
    fn drop(&mut self) {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        
        windows_route::sys::remove_bypass_routes(&self.bypass_routes);
        tracing::info!("Removed {} bypass routes.", self.bypass_routes.len());

        let is_kill_switch = self.kill_switch;
        let _ = std::thread::spawn(move || {
            let _ = Command::new("netsh")
                .creation_flags(CREATE_NO_WINDOW)
                .args(["advfirewall", "firewall", "delete", "rule", "name=OSTP Tunnel In"])
                .output();
            let _ = Command::new("netsh")
                .creation_flags(CREATE_NO_WINDOW)
                .args(["advfirewall", "firewall", "delete", "rule", "name=OSTP Tunnel Out"])
                .output();
            let _ = Command::new("netsh")
                .creation_flags(CREATE_NO_WINDOW)
                .args(["interface", "ipv4", "set", "dnsservers",
                       "name=ostp_tun", "source=dhcp"])
                .output();
            if is_kill_switch {
                let _ = Command::new("route")
                    .creation_flags(CREATE_NO_WINDOW)
                    .args(["delete", "0.0.0.0", "mask", "0.0.0.0", "127.0.0.1"])
                    .output();
            }
        });
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
    let dev = tun::AsyncDevice::new(dev).map_err(|e| anyhow!("TUN device async failed: {}", e))?;
    tracing::info!("TUN device 'ostp_tun' created.");

    let current_exe = std::env::current_exe()?.to_string_lossy().into_owned();

    // A freshly created WinTun adapter can take several seconds to appear in
    // GetAdaptersAddresses (it only shows up once it has an operational IPv4
    // binding). The default route via the TUN is what actually captures
    // traffic, so this lookup is critical — give it a generous window
    // (~15s) before giving up rather than the previous 2s.
    let mut tun_index = None;
    for _ in 0..75 {
        if let Some(idx) = windows_route::sys::get_interface_index("ostp_tun") {
            tun_index = Some(idx);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    if let Some(idx) = tun_index {
        match windows_route::sys::add_ipv4_route(
            std::net::Ipv4Addr::new(0, 0, 0, 0),
            std::net::Ipv4Addr::new(0, 0, 0, 0),
            std::net::Ipv4Addr::new(10, 1, 0, 1),
            idx,
            5,
        ) {
            Ok(()) => tracing::info!("Default route via TUN (if_index={idx}, metric=5) added."),
            Err(e) => tracing::error!("Failed to add default route via TUN (if_index={idx}): {e} — traffic will NOT be captured."),
        }
    } else {
        tracing::error!("Could not find ostp_tun index in routing table after 15s — traffic will NOT be captured.");
    }

    let exe1 = current_exe.clone();
    let exe2 = current_exe.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let _ = Command::new("netsh")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["advfirewall", "firewall", "add", "rule",
                   "name=OSTP Tunnel In", "dir=in", "action=allow",
                   &format!("program={}", exe1)])
            .output();
        let _ = Command::new("netsh")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["advfirewall", "firewall", "add", "rule",
                   "name=OSTP Tunnel Out", "dir=out", "action=allow",
                   &format!("program={}", exe2)])
            .output();
        let _ = Command::new("netsh")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["interface", "ipv4", "set", "interface", "name=ostp_tun",
                   "routerdiscovery=disabled", "dadtransmits=0",
                   "managedaddress=disabled", "otherstateful=disabled"])
            .output();
    });

    if let Some(ref dns) = opts.dns_server {
        if !dns.is_empty() {
            let dns_clone = dns.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = Command::new("netsh")
                    .creation_flags(CREATE_NO_WINDOW)
                    .args(["interface", "ipv4", "set", "dnsservers",
                           "name=ostp_tun", "static", &dns_clone, "primary"])
                    .output();
            });
        }
    }

    if opts.kill_switch {
        tracing::info!("Kill Switch enabled: Adding metric 10 blackhole route to prevent leakage");
        let _ = tokio::task::spawn_blocking(move || {
            let _ = Command::new("route")
                .creation_flags(CREATE_NO_WINDOW)
                .args(["add", "0.0.0.0", "mask", "0.0.0.0", "127.0.0.1", "metric", "10", "if", "1"])
                .output();
        });
    }

    Ok(OstpTunInterface {
        device: dev,
        guard: Box::new(WindowsRouteGuard {
            bypass_routes,
            kill_switch: opts.kill_switch,
        }),
    })
}
