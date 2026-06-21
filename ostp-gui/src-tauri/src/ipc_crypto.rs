// Re-export the shared IPC crypto from ostp-client so that GUI and tun-helper
// always use identical encrypt/decrypt logic.
pub use ostp_client::ipc_crypto::{derive_key, IpcCrypto};
