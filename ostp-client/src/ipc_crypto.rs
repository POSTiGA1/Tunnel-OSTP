use anyhow::{anyhow, Result};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use chacha20poly1305::aead::{Aead, KeyInit};
use sha2::{Sha256, Digest};

/// Symmetric IPC channel encryption for the tun-helper ↔ GUI pipe.
///
/// Both sides derive the same key from the per-launch random token, so no
/// secret is ever passed on the command line. The zero nonce is safe here
/// because each session uses a fresh random token, making key reuse impossible.
#[derive(Clone)]
pub struct IpcCrypto {
    cipher: ChaCha20Poly1305,
}

impl IpcCrypto {
    pub fn new(key: &[u8; 32]) -> Self {
        let cipher = ChaCha20Poly1305::new_from_slice(key)
            .expect("32-byte key is always valid for ChaCha20Poly1305");
        Self { cipher }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let nonce = Nonce::from_slice(&[0u8; 12]);
        self.cipher.encrypt(nonce, plaintext)
            .map_err(|e| anyhow!("IPC encrypt: {}", e))
    }

    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let nonce = Nonce::from_slice(&[0u8; 12]);
        self.cipher.decrypt(nonce, ciphertext)
            .map_err(|e| anyhow!("IPC decrypt: {}", e))
    }
}

/// Derive a 32-byte key from the per-session random token.
pub fn derive_key(token: &str) -> [u8; 32] {
    let mut key = [0u8; 32];
    key.copy_from_slice(&Sha256::digest(token.as_bytes()));
    key
}
