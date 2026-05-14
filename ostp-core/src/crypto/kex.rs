use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use x25519_dalek::{EphemeralSecret, PublicKey};

#[derive(Debug, Clone)]
pub struct HybridSharedSecret {
    pub x25519_pubkey: [u8; 32],
    pub pq_ciphertext: Vec<u8>,
    pub combined_secret: [u8; 32],
}

pub trait KeyExchange {
    fn client_kex() -> HybridSharedSecret;
}

pub struct HybridKex;

impl HybridKex {
    pub fn client_offer() -> HybridSharedSecret {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let pubkey = PublicKey::from(&secret);

        // Placeholder PQ ciphertext. Replace with ML-KEM encapsulation output.
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

impl KeyExchange for HybridKex {
    fn client_kex() -> HybridSharedSecret {
        Self::client_offer()
    }
}
