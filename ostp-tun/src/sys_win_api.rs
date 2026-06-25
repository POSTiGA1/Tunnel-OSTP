use anyhow::Result;
use windows::core::{BSTR, GUID};
use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::NetworkManagement::WindowsFirewall::{
    INetFwPolicy2, INetFwRule, NetFwPolicy2, NetFwRule, NET_FW_ACTION_ALLOW,
    NET_FW_PROFILE2_ALL, NET_FW_RULE_DIR_IN, NET_FW_RULE_DIR_OUT,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::NetworkManagement::IpHelper::{
    CreateIpForwardEntry, DeleteIpForwardEntry, MIB_IPFORWARDROW,
};
use std::net::Ipv4Addr;

fn init_com() -> Result<()> {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if hr.is_err() && hr.0 != windows::Win32::Foundation::RPC_E_CHANGED_MODE.0 {
            return Err(anyhow::anyhow!("CoInitializeEx failed: {}", hr));
        }
    }
    Ok(())
}

pub fn add_firewall_rules(exe_path: &str) -> Result<()> {
    init_com()?;
    unsafe {
        let policy: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER)?;

        let rules = policy.Rules()?;

        // Rule IN
        let rule_in: INetFwRule = CoCreateInstance(&NetFwRule, None, CLSCTX_INPROC_SERVER)?;
        rule_in.SetName(&BSTR::from("OSTP Tunnel In"))?;
        rule_in.SetApplicationName(&BSTR::from(exe_path))?;
        rule_in.SetAction(NET_FW_ACTION_ALLOW)?;
        rule_in.SetDirection(NET_FW_RULE_DIR_IN)?;
        rule_in.SetProfiles(NET_FW_PROFILE2_ALL.0)?;
        rules.Add(&rule_in)?;

        // Rule OUT
        let rule_out: INetFwRule = CoCreateInstance(&NetFwRule, None, CLSCTX_INPROC_SERVER)?;
        rule_out.SetName(&BSTR::from("OSTP Tunnel Out"))?;
        rule_out.SetApplicationName(&BSTR::from(exe_path))?;
        rule_out.SetAction(NET_FW_ACTION_ALLOW)?;
        rule_out.SetDirection(NET_FW_RULE_DIR_OUT)?;
        rule_out.SetProfiles(NET_FW_PROFILE2_ALL.0)?;
        rules.Add(&rule_out)?;
    }
    Ok(())
}

pub fn remove_firewall_rules() -> Result<()> {
    init_com()?;
    unsafe {
        let policy: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER)?;
        let rules = policy.Rules()?;
        let _ = rules.Remove(&BSTR::from("OSTP Tunnel In"));
        let _ = rules.Remove(&BSTR::from("OSTP Tunnel Out"));
    }
    Ok(())
}

// Minimal implementation of Kill Switch (blackhole route to 127.0.0.1) using WinAPI
pub fn set_kill_switch_route(enable: bool) -> Result<()> {
    // 0.0.0.0/0 -> 127.0.0.1 with metric 10 and if_index 1 (Loopback)
    let mut row: MIB_IPFORWARDROW = unsafe { std::mem::zeroed() };
    row.dwForwardDest = 0;
    row.dwForwardMask = 0;
    row.dwForwardPolicy = 0;
    row.dwForwardNextHop = u32::from_ne_bytes(Ipv4Addr::new(127, 0, 0, 1).octets());
    row.dwForwardIfIndex = 1; // Loopback interface
    row.Anonymous1.dwForwardType = 3; // MIB_IPROUTE_TYPE_INDIRECT
    row.Anonymous2.dwForwardProto = 3; // MIB_IPPROTO_NETMGMT
    row.dwForwardAge = 0;
    row.dwForwardNextHopAS = 0;
    row.dwForwardMetric1 = 10;
    row.dwForwardMetric2 = !0;
    row.dwForwardMetric3 = !0;
    row.dwForwardMetric4 = !0;
    row.dwForwardMetric5 = !0;

    unsafe {
        if enable {
            let res = WIN32_ERROR(CreateIpForwardEntry(&row));
            if res != ERROR_SUCCESS && res != windows::Win32::Foundation::ERROR_OBJECT_ALREADY_EXISTS {
                return Err(anyhow::anyhow!("CreateIpForwardEntry failed: {}", res.0));
            }
        } else {
            let res = WIN32_ERROR(DeleteIpForwardEntry(&row));
            if res != ERROR_SUCCESS && res != windows::Win32::Foundation::ERROR_NOT_FOUND {
                return Err(anyhow::anyhow!("DeleteIpForwardEntry failed: {}", res.0));
            }
        }
    }
    Ok(())
}

// DNS setting using WMI (requires wmi crate) or IP Helper API.
// SetInterfaceDnsSettings was added in 1607, let's use it.
pub fn set_dns_servers(adapter_luid: u64, dns: &str) -> Result<()> {
    use windows::Win32::NetworkManagement::IpHelper::{
        SetInterfaceDnsSettings, DNS_INTERFACE_SETTINGS,
    };
    use std::os::windows::ffi::OsStrExt;

    let dns_wstr: Vec<u16> = std::ffi::OsStr::new(dns)
        .encode_wide()
        .chain(Some(0))
        .collect();

    let settings = DNS_INTERFACE_SETTINGS {
        Version: 1, // DNS_INTERFACE_SETTINGS_VERSION1
        Flags: 1, // DNS_SETTING_IPV4
        Domain: windows::core::PWSTR::null(),
        NameServer: windows::core::PWSTR::from_raw(dns_wstr.as_ptr() as *mut _),
        SearchList: windows::core::PWSTR::null(),
        RegistrationEnabled: 0,
        RegisterAdapterName: 0,
        EnableLLMNR: 0,
        QueryAdapterName: 0,
        ProfileNameServer: windows::core::PWSTR::null(),
    };

    let luid = windows::Win32::NetworkManagement::Ndis::NET_LUID_LH { Value: adapter_luid };
    let _guid = GUID::zeroed(); // We can pass zeroed GUID and just use LUID? Wait, SetInterfaceDnsSettings requires GUID.
    
    // Actually, setting DNS via SetInterfaceDnsSettings requires the interface GUID, which we can get from ConvertInterfaceLuidToGuid.
    unsafe {
        let mut if_guid = GUID::zeroed();
        let err = windows::Win32::NetworkManagement::IpHelper::ConvertInterfaceLuidToGuid(&luid, &mut if_guid);
        if err != ERROR_SUCCESS {
            return Err(anyhow::anyhow!("ConvertInterfaceLuidToGuid failed: {}", err.0));
        }

        let err = SetInterfaceDnsSettings(if_guid, &settings);
        if err != ERROR_SUCCESS {
            return Err(anyhow::anyhow!("SetInterfaceDnsSettings failed: {}", err.0));
        }
    }

    Ok(())
}
