use crate::field::Hex;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn random_bytes<const N: usize>() -> Option<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes)
}

fn fallback<const N: usize>(tag: &str) -> [u8; N] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(tag.as_bytes());
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; N];
    out.copy_from_slice(&digest[..N]);
    out
}

pub fn new_boot() -> Hex {
    let bytes = random_bytes::<4>().unwrap_or_else(|| fallback::<4>("boot"));
    Hex::from_bytes(&bytes)
}

pub fn new_op_id() -> Hex {
    let bytes = random_bytes::<8>().unwrap_or_else(|| fallback::<8>("op"));
    Hex::from_bytes(&bytes)
}
