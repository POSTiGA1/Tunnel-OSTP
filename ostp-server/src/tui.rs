use std::net::IpAddr;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum UiCommand {
    CreateClientKey,
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum UiEvent {
    PeerSeen { peer: IpAddr },
    Rx { peer: IpAddr, bytes: usize },
    Tx { peer: IpAddr, bytes: usize },
    UnauthorizedProbe { peer: IpAddr, bytes: usize, reason: String },
    KeyCreated { key: String },
    Log(String),
    KeyCount(usize),
}

/// No-op placeholder — TUI removed. Server always runs in headless mode.
pub async fn run_server_tui(
    _ui_event_rx: mpsc::UnboundedReceiver<UiEvent>,
    _ui_cmd_tx: mpsc::UnboundedSender<UiCommand>,
    _initial_key_count: usize,
    _peer_idle_timeout: std::time::Duration,
) -> anyhow::Result<()> {
    Ok(())
}
