use portable_atomic::{AtomicU64, AtomicU32, AtomicU8};

pub struct BridgeMetrics {
    pub bytes_sent: AtomicU64,
    pub bytes_recv: AtomicU64,
    pub connection_state: AtomicU8,
    pub rtt_ms: AtomicU32,
}

impl Default for BridgeMetrics {
    fn default() -> Self {
        Self {
            bytes_sent: portable_atomic::AtomicU64::new(0),
            bytes_recv: portable_atomic::AtomicU64::new(0),
            connection_state: portable_atomic::AtomicU8::new(0),
            rtt_ms: portable_atomic::AtomicU32::new(0),
        }
    }
}

pub fn set_socket_protector<F>(f: F)
where
    F: Fn(i32) -> bool + Send + Sync + 'static,
{
    // stub
}
