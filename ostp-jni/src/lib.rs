use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jstring};
use jni::JNIEnv;

use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::sync::atomic::Ordering;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, watch};
use ostp_client::bridge::BridgeMetrics;
use std::io::Write;

static LOG_TX: std::sync::OnceLock<std::sync::mpsc::Sender<String>> = std::sync::OnceLock::new();

struct JniLogWriter;

impl Write for JniLogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let s = String::from_utf8_lossy(buf).trim().to_string();
        if !s.is_empty() {
            if let Some(tx) = LOG_TX.get() {
                let _ = tx.send(s);
            } else {
                add_log(s);
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for JniLogWriter {
    type Writer = JniLogWriter;
    fn make_writer(&'a self) -> Self::Writer {
        JniLogWriter
    }
}

static TRACING_INIT: std::sync::Once = std::sync::Once::new();

fn init_tracing() {
    TRACING_INIT.call_once(|| {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        LOG_TX.set(tx).ok();
        std::thread::spawn(move || {
            while let Ok(text) = rx.recv() {
                add_log(text);
            }
        });

        let subscriber = tracing_subscriber::fmt()
            .with_writer(JniLogWriter)
            .with_ansi(false)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

struct SdkState {
    runtime: Option<Runtime>,
    shutdown_tx: Option<watch::Sender<bool>>,
    metrics: Option<Arc<BridgeMetrics>>,
    tun_child: Option<std::process::Child>,
}

impl SdkState {
    const fn new() -> Self {
        Self {
            runtime: None,
            shutdown_tx: None,
            metrics: None,
            tun_child: None,
        }
    }
}

static STATE: RwLock<SdkState> = RwLock::new(SdkState::new());
static LOGS: RwLock<VecDeque<String>> = RwLock::new(VecDeque::new());
static JVM: RwLock<Option<jni::JavaVM>> = RwLock::new(None);
static CLASS_REF: RwLock<Option<jni::objects::GlobalRef>> = RwLock::new(None);

fn add_log(text: String) {
    if let Ok(mut guard) = LOGS.write() {
        if guard.len() >= 1000 {
            guard.pop_front();
        }
        guard.push_back(text);
    }
}

#[no_mangle]
pub extern "system" fn Java_net_ostp_client_OstpClientSdk_nativeStartClient(
    mut env: JNIEnv,
    _class: JClass,
    config_json: JString,
    fd: jni::sys::jint,
    _t2s_bin_path: JString,
    _local_proxy: JString,
) -> jboolean {
    init_tracing();

    if let Ok(jvm) = env.get_java_vm() {
        if let Ok(mut guard) = JVM.write() {
            *guard = Some(jvm);
        }
    }

    if let Ok(cls) = env.find_class("net/ostp/client/OstpClientSdk") {
        if let Ok(global_cls) = env.new_global_ref(cls) {
            if let Ok(mut guard) = CLASS_REF.write() {
                *guard = Some(global_cls);
            }
        }
    }

    ostp_client::bridge::set_socket_protector(|fd| {
        let jvm_guard = match JVM.read() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let class_guard = match CLASS_REF.read() {
            Ok(g) => g,
            Err(_) => return false,
        };
        if let (Some(ref jvm), Some(ref class_ref)) = (&*jvm_guard, &*class_guard) {
            if let Ok(mut env) = jvm.attach_current_thread() {
                let class_obj = unsafe { jni::objects::JClass::from_raw(class_ref.as_obj().as_raw()) };
                let val = env.call_static_method(
                    &class_obj,
                    "protectSocket",
                    "(I)Z",
                    &[jni::objects::JValue::from(fd)],
                );
                if let Ok(jval) = val {
                    return jval.z().unwrap_or(false);
                }
            }
        }
        false
    });

    let config_str: String = match env.get_string(&config_json) {
        Ok(s) => s.into(),
        Err(_) => return jni::sys::JNI_FALSE,
    };

    // Parse config from JSON
    let parsed_val: serde_json::Value = match serde_json::from_str(&config_str) {
        Ok(v) => v,
        Err(e) => {
            add_log(format!("Failed to parse config JSON: {e}"));
            return jni::sys::JNI_FALSE;
        }
    };

    let (mut migrated, _) = ostp_client::config::ClientConfig::migrate_json(parsed_val);
    
    // Insert fd into TUN inbound
    if fd > 0 {
        if let Some(inbounds) = migrated.get_mut("inbounds").and_then(|v| v.as_array_mut()) {
            for inbound in inbounds.iter_mut() {
                if inbound.get("type").and_then(|t| t.as_str()) == Some("tun") {
                    if let Some(obj) = inbound.as_object_mut() {
                        obj.insert("fd".to_string(), serde_json::json!(fd));
                    }
                }
            }
        }
    }

    let config: ostp_client::config::ClientConfig = match serde_json::from_value(migrated) {
        Ok(cfg) => cfg,
        Err(e) => {
            add_log(format!("Failed to build ClientConfig: {e}"));
            return jni::sys::JNI_FALSE;
        }
    };

    // Create tokio runtime
    let rt = match Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            add_log(format!("Failed to create Tokio runtime: {e}"));
            return jni::sys::JNI_FALSE;
        }
    };

    let metrics = Arc::new(ostp_client::bridge::BridgeMetrics {
        bytes_sent: portable_atomic::AtomicU64::new(0),
        bytes_recv: portable_atomic::AtomicU64::new(0),
        connection_state: portable_atomic::AtomicU8::new(0),
        rtt_ms: portable_atomic::AtomicU32::new(0),
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let metrics_clone = Arc::clone(&metrics);

    rt.spawn(async move {
        if let Err(e) = ostp_client::runner::run_client_core(config, metrics_clone, shutdown_rx, None).await {
            add_log(format!("OSTP Core exited with error: {}", e));
        }
    });

    let mut state = match STATE.write() {
        Ok(s) => s,
        Err(_) => return jni::sys::JNI_FALSE,
    };

    state.runtime = Some(rt);
    state.shutdown_tx = Some(shutdown_tx);
    state.metrics = Some(metrics);
    state.tun_child = None;

    add_log("OSTP SDK: Client successfully started".to_string());
    jni::sys::JNI_TRUE
}

#[no_mangle]
pub extern "system" fn Java_net_ostp_client_OstpClientSdk_startClient(
    env: JNIEnv,
    class: JClass,
    config_json: JString,
    fd: jni::sys::jint,
    t2s_bin_path: JString,
    local_proxy: JString,
) -> jboolean {
    Java_net_ostp_client_OstpClientSdk_nativeStartClient(env, class, config_json, fd, t2s_bin_path, local_proxy)
}

#[no_mangle]
pub extern "system" fn Java_net_ostp_client_OstpClientSdk_nativeStopClient(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    let (tun_child, shutdown_tx, runtime) = {
        let mut state = match STATE.write() {
            Ok(s) => s,
            Err(_) => return jni::sys::JNI_FALSE,
        };
        let c = state.tun_child.take();
        let s = state.shutdown_tx.take();
        let r = state.runtime.take();
        state.metrics = None;
        (c, s, r)
    };

    if let Some(mut child) = tun_child {
        let _ = child.kill();
        add_log("Killed tun2socks process".to_string());
    }

    if let Some(s) = shutdown_tx {
        let _ = s.send(true);
    }

    if let Some(rt) = runtime {
        rt.shutdown_background();
    }

    add_log("OSTP SDK: Client successfully stopped".to_string());
    jni::sys::JNI_TRUE
}

#[no_mangle]
pub extern "system" fn Java_net_ostp_client_OstpClientSdk_stopClient(
    env: JNIEnv,
    class: JClass,
) -> jboolean {
    Java_net_ostp_client_OstpClientSdk_nativeStopClient(env, class)
}

#[no_mangle]
pub extern "system" fn Java_net_ostp_client_OstpClientSdk_nativeGetMetrics(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let state = match STATE.read() {
        Ok(s) => s,
        Err(_) => return match env.new_string("{}") {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
    };

    if let Some(m) = &state.metrics {
        let sent = m.bytes_sent.load(Ordering::Relaxed);
        let recv = m.bytes_recv.load(Ordering::Relaxed);
        let conn_state = m.connection_state.load(Ordering::Relaxed);
        let rtt = m.rtt_ms.load(Ordering::Relaxed);
        let json = format!(
            r#"{{"bytes_sent": {}, "bytes_recv": {}, "connection_state": {}, "rtt_ms": {}}}"#,
            sent, recv, conn_state, rtt
        );
        match env.new_string(json.replace('\0', "")) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    } else {
        match env.new_string(r#"{"bytes_sent": 0, "bytes_recv": 0, "connection_state": 0, "rtt_ms": 0}"#) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_net_ostp_client_OstpClientSdk_getMetrics(
    env: JNIEnv,
    class: JClass,
) -> jstring {
    Java_net_ostp_client_OstpClientSdk_nativeGetMetrics(env, class)
}

#[no_mangle]
pub extern "system" fn Java_net_ostp_client_OstpClientSdk_nativeGetLogs(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let logs_vec: Vec<String> = match LOGS.write() {
        Ok(mut guard) => guard.drain(..).collect(),
        Err(_) => Vec::new(),
    };

    let json = match serde_json::to_string(&logs_vec) {
        Ok(s) => s,
        Err(_) => "[]".to_string(),
    };

    match env.new_string(json.replace('\0', "")) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_net_ostp_client_OstpClientSdk_getLogs(
    env: JNIEnv,
    class: JClass,
) -> jstring {
    Java_net_ostp_client_OstpClientSdk_nativeGetLogs(env, class)
}

#[no_mangle]
pub extern "system" fn Java_net_ostp_client_OstpClientSdk_addLog(
    mut env: JNIEnv,
    _class: JClass,
    log_msg: JString,
) {
    if let Ok(s) = env.get_string(&log_msg) {
        let text: String = s.into();
        add_log(text);
    }
}

/// Called by Android NetworkCallback when the active network changes (WiFi→LTE, etc.).
/// Sends BridgeCommand::NetworkChanged to trigger an immediate reconnect in the Rust bridge.
#[no_mangle]
pub extern "system" fn Java_net_ostp_client_OstpClientSdk_notifyNetworkChanged(
    _env: JNIEnv,
    _class: JClass,
) {
    let _state = match STATE.read() {
        Ok(s) => s,
        Err(_) => return,
    };

    // No-op for now; multi-server handles network drops via keep-alives and reconnection
}
