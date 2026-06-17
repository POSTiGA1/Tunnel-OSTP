use ed25519_dalek::{VerifyingKey, Signature, Verifier};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

const PUBLIC_KEY_BYTES: [u8; 32] = [
    195, 200, 121, 254, 102, 179, 130, 80, 88, 252, 123, 193, 254, 31, 64, 66, 13, 60, 192, 132, 166, 240, 21, 86, 85, 27, 230, 207, 129, 192, 121, 225
];

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LicensePayload {
    pub issued_at: u64,
    pub expires_at: u64,
    pub bind_host: String,
    pub features: Vec<String>,
}

#[derive(Debug)]
pub enum LicenseError {
    InvalidFormat,
    InvalidSignature,
    Expired,
    InvalidHost,
    DecodeError,
}

pub fn verify_license(license_key: &str, current_host: &str) -> Result<LicensePayload, LicenseError> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let parts: Vec<&str> = license_key.split('.').collect();
    if parts.len() != 2 {
        return Err(LicenseError::InvalidFormat);
    }

    let payload_bytes = b64.decode(parts[0]).map_err(|_| LicenseError::DecodeError)?;
    let sig_bytes = b64.decode(parts[1]).map_err(|_| LicenseError::DecodeError)?;

    if sig_bytes.len() != 64 {
        return Err(LicenseError::InvalidSignature);
    }

    let public_key = VerifyingKey::from_bytes(&PUBLIC_KEY_BYTES).map_err(|_| LicenseError::InvalidSignature)?;
    let signature = Signature::from_slice(sig_bytes.as_slice()).map_err(|_| LicenseError::InvalidSignature)?;

    if public_key.verify(&payload_bytes, &signature).is_err() {
        return Err(LicenseError::InvalidSignature);
    }

    let payload: LicensePayload = serde_json::from_slice(&payload_bytes).map_err(|_| LicenseError::DecodeError)?;

    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
    if now > payload.expires_at {
        return Err(LicenseError::Expired);
    }

    if payload.bind_host != current_host && payload.bind_host != "0.0.0.0" && payload.bind_host != "*" {
        return Err(LicenseError::InvalidHost);
    }

    Ok(payload)
}
