use anyhow::{anyhow, Result};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use chacha20poly1305::aead::{Aead, KeyInit};
use sha2::{Sha256, Digest};

pub struct IpcCrypto {
    cipher: ChaCha20Poly1305,
    nonce: [u8; 12],
}

impl IpcCrypto {
    pub fn new(key: &[u8; 32]) -> Self {
        let cipher = ChaCha20Poly1305::new_from_slice(key)
            .expect("valid key size");
        let nonce = [0u8; 12];
        Self { cipher, nonce }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let nonce = Nonce::from_slice(&self.nonce);
        let ciphertext = self.cipher.encrypt(nonce, plaintext)
            .map_err(|e| anyhow!("Encryption failed: {}", e))?;
        Ok(ciphertext)
    }

    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let nonce = Nonce::from_slice(&self.nonce);
        let plaintext = self.cipher.decrypt(nonce, ciphertext)
            .map_err(|e| anyhow!("Decryption failed: {}", e))?;
        Ok(plaintext)
    }
}

pub fn derive_key(token: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}
