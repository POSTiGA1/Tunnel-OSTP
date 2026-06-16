use portable_atomic::{AtomicU64, AtomicU32, AtomicU8};

pub struct BridgeMetrics {
    pub bytes_sent: AtomicU64,
    pub bytes_recv: AtomicU64,
    pub connection_state: AtomicU8,
    pub rtt_ms: AtomicU32,
}

pub fn set_socket_protector<F>(f: F)
where
    F: Fn(i32) -> bool + Send + Sync + 'static,
{
    // stub
}
