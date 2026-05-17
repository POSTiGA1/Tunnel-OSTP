// =============================================================================
// OSTP Hybrid Key Exchange — STUB / NOT IN USE
// =============================================================================
//
// This module is a placeholder for future post-quantum key exchange.
// The actual key exchange is handled by the Noise NNpsk0 handshake in noise.rs.
//
// When ML-KEM (CRYSTALS-Kyber) support is added, this module will provide:
//   1. X25519 ephemeral DH  (classical security)
//   2. ML-KEM-768 encapsulation  (post-quantum security)
//   3. Combined shared secret = SHA-256(x25519_secret || ml_kem_secret)
//
// Until then, DO NOT use this module in production — it provides zero
// post-quantum security. The Noise handshake in noise.rs is the only
// active key exchange mechanism.
// =============================================================================

#![allow(dead_code)]

use sha2::{Digest, Sha256};

/// Placeholder shared secret output.
/// NOT USED by the protocol — provided for future API compatibility only.
#[derive(Debug, Clone)]
pub struct HybridSharedSecret {
    pub x25519_pubkey: [u8; 32],
    pub pq_ciphertext: Vec<u8>,
    pub combined_secret: [u8; 32],
}

/// Placeholder hybrid key exchange.  
/// The PQ component is a no-op stub. See module-level documentation.
pub struct HybridKex;

impl HybridKex {
    /// Generate a hybrid key exchange offer.
    ///
    /// # Security Warning
    /// The post-quantum component is a **stub** — `pq_ciphertext` is all zeros.
    /// This function exists solely for API scaffolding. Do not rely on it for
    /// post-quantum security.
    pub fn client_offer() -> HybridSharedSecret {
        use rand::rngs::OsRng;
        use x25519_dalek::{EphemeralSecret, PublicKey};

        let secret = EphemeralSecret::random_from_rng(OsRng);
        let pubkey = PublicKey::from(&secret);

        // TODO: Replace with ML-KEM-768 encapsulation (crate `ml-kem`)
        let pq_ciphertext = vec![0_u8; 1088];

        let mut hasher = Sha256::new();
        hasher.update(pubkey.as_bytes());
        hasher.update(&pq_ciphertext);
        let digest = hasher.finalize();

        let mut combined_secret = [0_u8; 32];
        combined_secret.copy_from_slice(&digest[..32]);

        HybridSharedSecret {
            x25519_pubkey: *pubkey.as_bytes(),
            pq_ciphertext,
            combined_secret,
        }
    }
}
