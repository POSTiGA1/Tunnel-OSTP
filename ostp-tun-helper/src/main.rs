// ostp-tun-helper/src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use hex;
use ostp_client::ipc_crypto::{derive_key, IpcCrypto};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{watch, Mutex};
use portable_atomic::Ordering;

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase")]
enum GuiCmd {
    Start { config: String, token: String },
    Reload { config: String, token: String },
    Stop { token: String },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[allow(dead_code)]
enum HelperMsg {
    Status { value: u8 },
    Log { message: String },
    Metrics { bytes_sent: u64, bytes_recv: u64, rtt_ms: u32 },
    Error { message: String },
}

struct TunnelState {
    shutdown_tx: Option<watch::Sender<bool>>,
    config_tx: Option<watch::Sender<ostp_client::config::ClientConfig>>,
    metrics: Option<Arc<ostp_client::bridge::BridgeMetrics>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    ostp_client::logging::setup_panic_hook();
    let _log_guard = ostp_client::logging::init_tracing("info", "ostp-helper", env!("CARGO_PKG_VERSION"));

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let _ = std::env::set_current_dir(dir);
        }
    }

    let mut expected_token = std::env::var("OSTP_TUN_TOKEN").unwrap_or_default();
    let mut port = 53211u16;
    let args: Vec<String> = std::env::args().collect();
    for i in 1..args.len() {
        if args[i] == "--port" && i + 1 < args.len() {
            port = args[i + 1].parse().unwrap_or(53211);
        }
        if args[i] == "--token-file" && i + 1 < args.len() {
            let path = &args[i + 1];
            if let Ok(content) = std::fs::read_to_string(path) {
                expected_token = content.trim().to_string();
                let _ = std::fs::remove_file(path);
            }
        }
    }

    tracing::info!("helper started (TCP mode)");

    if expected_token.is_empty() {
        tracing::error!("auth token is required (--token-file or OSTP_TUN_TOKEN)");
        return Err(anyhow::anyhow!("auth token is required"));
    }

    if let Err(e) = run_server(expected_token, port).await {
        tracing::error!("fatal: {}", e);
    }
    tracing::info!("helper exiting");
    Ok(())
}

async fn run_server(expected_token: String, port: u16) -> Result<()> {
    let state = Arc::new(Mutex::new(TunnelState {
        shutdown_tx: None,
        config_tx: None,
        metrics: None,
    }));

    let ipc_key = derive_key(&expected_token);
    let crypto = IpcCrypto::new(&ipc_key);

    let bind_addr = format!("127.0.0.1:{}", port);
    tracing::info!("binding to {}", bind_addr);
    let listener = TcpListener::bind(&bind_addr).await.map_err(|e| {
        tracing::error!("bind failed: {}", e);
        e
    })?;
    tracing::info!("listening, waiting for GUI connection");

    let (socket, _) = match tokio::time::timeout(Duration::from_secs(60), listener.accept()).await {
        Ok(Ok(s)) => s,
        _ => {
            tracing::warn!("no connection from GUI within 60s, exiting");
            return Ok(());
        }
    };

    tracing::info!("GUI connected");

    let (reader_half, writer_half) = tokio::io::split(socket);
    let writer = Arc::new(Mutex::new(writer_half));
    let mut reader = BufReader::new(reader_half);

    let send_msg = {
        let writer = writer.clone();
        let crypto = crypto.clone();
        move |msg: HelperMsg| {
            let writer = writer.clone();
            let crypto = crypto.clone();
            let json = serde_json::to_string(&msg).unwrap_or_default();
            tokio::spawn(async move {
                match crypto.encrypt(json.as_bytes()) {
                    Ok(enc) => {
                        let line = format!("{}\n", hex::encode(&enc));
                        let mut w = writer.lock().await;
                        let _ = w.write_all(line.as_bytes()).await;
                    }
                    Err(e) => tracing::error!("send_msg encrypt failed: {}", e),
                }
            });
        }
    };

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await.unwrap_or(0);
        if n == 0 {
            tracing::info!("GUI disconnected, stopping tunnel");
            let mut st = state.lock().await;
            if let Some(tx) = st.shutdown_tx.take() {
                let _ = tx.send(true);
            }
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        // Decrypt the hex-encoded encrypted command from the GUI
        let decrypted_json = match hex::decode(trimmed)
            .ok()
            .and_then(|enc| crypto.decrypt(&enc).ok())
            .and_then(|dec| String::from_utf8(dec).ok())
        {
            Some(s) => s,
            None => {
                tracing::warn!("received undecodable command, ignoring");
                continue;
            }
        };

        let cmd: GuiCmd = match serde_json::from_str(&decrypted_json) {
            Ok(c) => c,
            Err(e) => {
                send_msg(HelperMsg::Error { message: format!("bad command: {}", e) });
                continue;
            }
        };

        match cmd {
            GuiCmd::Start { config, token } => {
                if token != expected_token {
                    tracing::warn!("START command with invalid token");
                    send_msg(HelperMsg::Error { message: "invalid authorization token".to_string() });
                    continue;
                }
                tracing::info!("received START command");
                {
                    let mut st = state.lock().await;
                    if let Some(tx) = st.shutdown_tx.take() {
                        let _ = tx.send(true);
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }

                let cfg: ostp_client::config::ClientConfig = match serde_json::from_str(&config) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("config parse error: {}", e);
                        send_msg(HelperMsg::Error { message: format!("config parse error: {}", e) });
                        continue;
                    }
                };

                let metrics = Arc::new(ostp_client::bridge::BridgeMetrics {
                    bytes_sent: portable_atomic::AtomicU64::new(0),
                    bytes_recv: portable_atomic::AtomicU64::new(0),
                    connection_state: portable_atomic::AtomicU8::new(0),
                    rtt_ms: portable_atomic::AtomicU32::new(0),
                });

                let (shutdown_tx, shutdown_rx) = watch::channel(false);
                let (config_tx, config_rx) = watch::channel(cfg.clone());

                {
                    let mut st = state.lock().await;
                    st.shutdown_tx = Some(shutdown_tx);
                    st.config_tx = Some(config_tx);
                    st.metrics = Some(metrics.clone());
                }

                let metrics_for_runner = metrics.clone();
                let writer_for_err = writer.clone();
                let crypto_for_err = crypto.clone();
                let shutdown_rx_for_core = shutdown_rx.clone();
                tokio::spawn(async move {
                    tracing::info!("starting tunnel core");
                    match ostp_client::runner::run_client_core(cfg, metrics_for_runner, shutdown_rx_for_core, Some(config_rx)).await {
                        Ok(_) => tracing::info!("tunnel core stopped normally"),
                        Err(e) => {
                            tracing::error!("tunnel core error: {}", e);
                            let json = serde_json::to_string(&HelperMsg::Error { message: e.to_string() })
                                .unwrap_or_default();
                            if let Ok(enc) = crypto_for_err.encrypt(json.as_bytes()) {
                                let mut w = writer_for_err.lock().await;
                                let _ = w.write_all(format!("{}\n", hex::encode(&enc)).as_bytes()).await;
                            }
                        }
                    }
                });

                let writer_tick = writer.clone();
                let crypto_tick = crypto.clone();
                let metrics_tick = metrics.clone();
                let mut shutdown_rx_tick = shutdown_rx.clone();
                tokio::spawn(async move {
                    let mut last_state = 99u8;
                    loop {
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                            _ = shutdown_rx_tick.changed() => {
                                if *shutdown_rx_tick.borrow() { break; }
                            }
                        }

                        let cs = metrics_tick.connection_state.load(Ordering::Relaxed);
                        let sent = metrics_tick.bytes_sent.load(Ordering::Relaxed);
                        let recv = metrics_tick.bytes_recv.load(Ordering::Relaxed);
                        let rtt = metrics_tick.rtt_ms.load(Ordering::Relaxed);

                        let mut msgs: Vec<HelperMsg> = Vec::new();
                        if cs != last_state {
                            last_state = cs;
                            msgs.push(HelperMsg::Status { value: cs });
                        }
                        msgs.push(HelperMsg::Metrics { bytes_sent: sent, bytes_recv: recv, rtt_ms: rtt });

                        let mut w = writer_tick.lock().await;
                        for msg in msgs {
                            let json = serde_json::to_string(&msg).unwrap_or_default();
                            if let Ok(enc) = crypto_tick.encrypt(json.as_bytes()) {
                                if w.write_all(format!("{}\n", hex::encode(&enc)).as_bytes()).await.is_err() {
                                    return;
                                }
                            }
                        }
                        drop(w);
                    }
                });

                send_msg(HelperMsg::Status { value: 1 });
            }
            GuiCmd::Reload { config, token } => {
                if token != expected_token {
                    send_msg(HelperMsg::Error { message: "invalid authorization token".to_string() });
                    continue;
                }
                tracing::info!("received RELOAD command");

                let cfg: ostp_client::config::ClientConfig = match serde_json::from_str(&config) {
                    Ok(c) => c,
                    Err(e) => {
                        send_msg(HelperMsg::Error { message: format!("config parse error during reload: {}", e) });
                        continue;
                    }
                };

                {
                    let st = state.lock().await;
                    if let Some(tx) = &st.config_tx {
                        let _ = tx.send(cfg);
                        tracing::info!("config sent to running core for hot-reload");
                    }
                }

                send_msg(HelperMsg::Status { value: 1 });
            }
            GuiCmd::Stop { token } => {
                if token != expected_token {
                    tracing::warn!("STOP command with invalid token");
                    send_msg(HelperMsg::Error { message: "invalid authorization token".to_string() });
                    continue;
                }
                tracing::info!("received STOP command");
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
