use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::{
    digest,
    rand::{SecureRandom, SystemRandom},
};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn random() -> Result<String, String> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "system randomness unavailable")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(super) fn hash(secret: &str) -> Vec<u8> {
    digest::digest(&digest::SHA256, secret.as_bytes())
        .as_ref()
        .to_vec()
}
