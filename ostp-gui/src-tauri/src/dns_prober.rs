use ostp_core::dns_prober::{run_dns_prober as core_run_dns_prober, DnsProbeResult};

#[tauri::command]
pub async fn run_dns_prober(domain: String) -> Result<Vec<DnsProbeResult>, String> {
    core_run_dns_prober(&domain).await
}
