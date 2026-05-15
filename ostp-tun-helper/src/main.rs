// ostp-tun-helper/src/main.rs
//
// Privileged helper for TUN mode. Runs with Administrator rights.
// Communicates with ostp-gui via a named pipe IPC channel.
//
// Protocol over the named pipe (newline-delimited JSON):
//   GUI -> Helper: {"cmd":"start","config":<config json string>}
//   GUI -> Helper: {"cmd":"stop"}
//   Helper -> GUI: {"type":"status","value":0|1|2}       (0=stopped,1=connecting,2=connected)
//   Helper -> GUI: {"type":"log","message":"..."}
//   Helper -> GUI: {"type":"metrics","bytes_sent":N,"bytes_recv":N}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{watch, Mutex};
use portable_atomic::Ordering;

const PIPE_NAME: &str = r"\\.\pipe\ostp-tun-helper";

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase")]
enum GuiCmd {
    Start { config: String },
    Stop,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum HelperMsg {
    Status { value: u8 },
    Log { message: String },
    Metrics { bytes_sent: u64, bytes_recv: u64 },
    Error { message: String },
}

struct TunnelState {
    shutdown_tx: Option<watch::Sender<bool>>,
    metrics: Option<Arc<ostp_client::bridge::BridgeMetrics>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // The helper is always launched by the GUI. If no client connects within
    // 60 seconds, exit to avoid lingering admin processes.
    run_pipe_server().await
}

async fn run_pipe_server() -> Result<()> {
    use tokio::net::windows::named_pipe::{ServerOptions};

    let state = Arc::new(Mutex::new(TunnelState {
        shutdown_tx: None,
        metrics: None,
    }));

    // Create the named pipe server
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(PIPE_NAME)?;

    // Wait for GUI to connect (60 second timeout)
    let connect_timeout = tokio::time::timeout(
        Duration::from_secs(60),
        server.connect()
    ).await;

    let pipe = match connect_timeout {
        Ok(Ok(())) => server,
        _ => {
            // No client connected — exit silently
            return Ok(());
        }
    };

    let (reader_half, writer_half) = tokio::io::split(pipe);
    let writer = Arc::new(Mutex::new(writer_half));
    let mut reader = BufReader::new(reader_half);

    // Helper to send a message back to GUI
    let send_msg = {
        let writer = writer.clone();
        move |msg: HelperMsg| {
            let writer = writer.clone();
            let json = serde_json::to_string(&msg).unwrap_or_default();
            tokio::spawn(async move {
                let mut w = writer.lock().await;
                let _ = w.write_all(format!("{}\n", json).as_bytes()).await;
            });
        }
    };

    // Read commands from GUI
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await.unwrap_or(0);
        if n == 0 {
            // GUI disconnected — stop tunnel and exit
            let mut st = state.lock().await;
            if let Some(tx) = st.shutdown_tx.take() {
                let _ = tx.send(true);
            }
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let cmd: GuiCmd = match serde_json::from_str(trimmed) {
            Ok(c) => c,
            Err(e) => {
                send_msg(HelperMsg::Error { message: format!("Bad command: {}", e) });
                continue;
            }
        };

        match cmd {
            GuiCmd::Start { config } => {
                // Stop any existing tunnel first
                {
                    let mut st = state.lock().await;
                    if let Some(tx) = st.shutdown_tx.take() {
                        let _ = tx.send(true);
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }

                // Parse config
                let cfg: ostp_client::config::ClientConfig = match serde_json::from_str(&config) {
                    Ok(c) => c,
                    Err(e) => {
                        send_msg(HelperMsg::Error { message: format!("Config parse error: {}", e) });
                        continue;
                    }
                };

                let metrics = Arc::new(ostp_client::bridge::BridgeMetrics {
                    bytes_sent: portable_atomic::AtomicU64::new(0),
                    bytes_recv: portable_atomic::AtomicU64::new(0),
                    connection_state: portable_atomic::AtomicU8::new(0),
                });

                let (shutdown_tx, shutdown_rx) = watch::channel(false);

                {
                    let mut st = state.lock().await;
                    st.shutdown_tx = Some(shutdown_tx);
                    st.metrics = Some(metrics.clone());
                }

                // Spawn the tunnel
                let metrics_for_runner = metrics.clone();
                let send_log = {
                    let writer = writer.clone();
                    move |msg: String| {
                        let writer = writer.clone();
                        let json = serde_json::to_string(&HelperMsg::Log { message: msg }).unwrap_or_default();
                        tokio::spawn(async move {
                            let mut w = writer.lock().await;
                            let _ = w.write_all(format!("{}\n", json).as_bytes()).await;
                        });
                    }
                };

                let writer_for_tick = writer.clone();
                let metrics_for_tick = metrics.clone();

                tokio::spawn(async move {
                    match ostp_client::runner::run_client_core(cfg, metrics_for_runner, shutdown_rx).await {
                        Ok(_) => {}
                        Err(e) => {
                            let json = serde_json::to_string(&HelperMsg::Error { message: e.to_string() }).unwrap_or_default();
                            let mut w = writer_for_tick.lock().await;
                            let _ = w.write_all(format!("{}\n", json).as_bytes()).await;
                        }
                    }
                });

                // Spawn a tick that forwards status + metrics to GUI every second
                let writer_tick = writer.clone();
                let metrics_tick = metrics_for_tick.clone();
                tokio::spawn(async move {
                    let mut last_state = 99u8;
                    loop {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        let cs = metrics_tick.connection_state.load(Ordering::Relaxed);
                        let sent = metrics_tick.bytes_sent.load(Ordering::Relaxed);
                        let recv = metrics_tick.bytes_recv.load(Ordering::Relaxed);

                        let mut w = writer_tick.lock().await;
                        // Only send status change events
                        if cs != last_state {
                            last_state = cs;
                            let json = serde_json::to_string(&HelperMsg::Status { value: cs }).unwrap_or_default();
                            if w.write_all(format!("{}\n", json).as_bytes()).await.is_err() { break; }
                        }
                        // Always send metrics
                        let json = serde_json::to_string(&HelperMsg::Metrics { bytes_sent: sent, bytes_recv: recv }).unwrap_or_default();
                        if w.write_all(format!("{}\n", json).as_bytes()).await.is_err() { break; }
                        drop(w);
                    }
                });

                send_msg(HelperMsg::Status { value: 1 });
            }

            GuiCmd::Stop => {
                let mut st = state.lock().await;
                if let Some(tx) = st.shutdown_tx.take() {
                    let _ = tx.send(true);
                }
                st.metrics = None;
                send_msg(HelperMsg::Status { value: 0 });
            }
        }
    }

    Ok(())
}
