use sha2::{Digest, Sha256};

pub fn derive_obfuscation_key(access_key: &[u8]) -> [u8; 8] {
    let mut hasher = Sha256::new();
    hasher.update(access_key);
    let result = hasher.finalize();
    let mut key = [0u8; 8];
    key.copy_from_slice(&result[0..8]);
    key
}

pub fn derive_psk(access_key: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(access_key);
    hasher.update(b"-ostp-psk-salt");
    let result = hasher.finalize();
    let mut psk = [0u8; 32];
    psk.copy_from_slice(&result);
    psk
}

pub fn obfuscate_packet_inplace(raw: &mut [u8], key: &[u8; 8], is_handshake: bool) {
    if !is_handshake && raw.len() >= 12 {
        // Data packet
        let mut session_id_bytes = [raw[0], raw[1], raw[2], raw[3]];
        let mut nonce_bytes = [
            raw[4], raw[5], raw[6], raw[7],
            raw[8], raw[9], raw[10], raw[11]
        ];

        // 1. Obfuscate nonce with derived key
        for i in 0..8 {
            nonce_bytes[i] ^= key[i];
        }

        // 2. Obfuscate session_id with the REAL (unobfuscated) nonce
        let real_nonce = u64::from_be_bytes([
            raw[4], raw[5], raw[6], raw[7],
            raw[8], raw[9], raw[10], raw[11]
        ]);
        let nonce_low_32 = (real_nonce & 0xFFFFFFFF) as u32;
        let nonce_low_bytes = nonce_low_32.to_be_bytes();

        for i in 0..4 {
            session_id_bytes[i] ^= nonce_low_bytes[i];
        }

        // Put them back
        raw[0..4].copy_from_slice(&session_id_bytes);
        raw[4..12].copy_from_slice(&nonce_bytes);
    } else if raw.len() >= 4 {
        // Handshake packet (XOR with key)
        for i in 0..4 {
            raw[i] ^= key[i % 8];
        }
    }
}

pub fn deobfuscate_packet_inplace(raw: &mut [u8], key: &[u8; 8], is_handshake: bool) {
    if !is_handshake && raw.len() >= 12 {
        // Data packet
        let mut nonce_bytes = [
            raw[4], raw[5], raw[6], raw[7],
            raw[8], raw[9], raw[10], raw[11]
        ];

        // 1. Recover real nonce by XORing with key
        for i in 0..8 {
            nonce_bytes[i] ^= key[i];
        }
        let real_nonce = u64::from_be_bytes(nonce_bytes);
        let nonce_low_32 = (real_nonce & 0xFFFFFFFF) as u32;
        let nonce_low_bytes = nonce_low_32.to_be_bytes();

        // 2. Recover session_id by XORing with recovered nonce
        let mut session_id_bytes = [raw[0], raw[1], raw[2], raw[3]];
        for i in 0..4 {
            session_id_bytes[i] ^= nonce_low_bytes[i];
        }

        // Put them back
        raw[0..4].copy_from_slice(&session_id_bytes);
        raw[4..12].copy_from_slice(&nonce_bytes);
    } else if raw.len() >= 4 {
        // Handshake packet
        for i in 0..4 {
            raw[i] ^= key[i % 8];
        }
    }
}
