#[cfg(target_os = "windows")]
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(target_os = "windows")]
#[link(name = "wininet")]
extern "system" {
    fn InternetSetOptionW(
        hInternet: *mut std::ffi::c_void,
        dwOption: u32,
        lpBuffer: *mut std::ffi::c_void,
        dwBufferLength: u32,
    ) -> i32;
}
#[cfg(target_os = "windows")]
const INTERNET_OPTION_SETTINGS_CHANGED: u32 = 39;
#[cfg(target_os = "windows")]
const INTERNET_OPTION_REFRESH: u32 = 37;

#[cfg(target_os = "windows")]
pub fn enable_windows_proxy(proxy_addr: &str) {
    tracing::info!("Enabling Windows system proxy: {}", proxy_addr);

    let result = Command::new("reg")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "add",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v", "ProxyEnable",
            "/t", "REG_DWORD",
            "/d", "1",
            "/f",
        ])
        .output();
    match result {
        Ok(out) if !out.status.success() => {
            tracing::error!("Failed to set ProxyEnable: {}", String::from_utf8_lossy(&out.stderr));
        }
        Err(e) => tracing::error!("Failed to execute reg.exe (ProxyEnable): {}", e),
        _ => {}
    }

    let result = Command::new("reg")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "add",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v", "ProxyServer",
            "/t", "REG_SZ",
            "/d", proxy_addr,
            "/f",
        ])
        .output();
    match result {
        Ok(out) if !out.status.success() => {
            tracing::error!("Failed to set ProxyServer: {}", String::from_utf8_lossy(&out.stderr));
        }
        Err(e) => tracing::error!("Failed to execute reg.exe (ProxyServer): {}", e),
        _ => {}
    }

    // Set bypass list to prevent proxy loop for localhost traffic
    let _ = Command::new("reg")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "add",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v", "ProxyOverride",
            "/t", "REG_SZ",
            "/d", "localhost;127.*;10.*;192.168.*;<local>",
            "/f",
        ])
        .output();

    refresh_wininet();
    tracing::info!("System proxy enabled successfully");
}

#[cfg(target_os = "windows")]
pub fn disable_windows_proxy() {
    tracing::info!("Disabling Windows system proxy");
    let _ = Command::new("reg")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "add",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v", "ProxyEnable",
            "/t", "REG_DWORD",
            "/d", "0",
            "/f",
        ])
        .output();

    refresh_wininet();
}

#[cfg(target_os = "windows")]
fn refresh_wininet() {
    unsafe {
        InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            std::ptr::null_mut(),
            0,
        );
        InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_REFRESH,
            std::ptr::null_mut(),
            0,
        );
    }
}

#[cfg(not(target_os = "windows"))]
pub fn enable_windows_proxy(_proxy_addr: &str) {}

#[cfg(not(target_os = "windows"))]
pub fn disable_windows_proxy() {}

pub struct WindowsProxyGuard {
    active: bool,
}

impl WindowsProxyGuard {
    pub fn enable(proxy_addr: &str) -> Self {
        enable_windows_proxy(proxy_addr);
        Self { active: true }
    }
}

impl Drop for WindowsProxyGuard {
    fn drop(&mut self) {
        if self.active {
            disable_windows_proxy();
        }
    }
}
