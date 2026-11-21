use sha2::{Digest, Sha256};
use std::fmt::Write;
use std::path::Path;

pub fn evidence_hash(salt: &[u8; 32], label: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(label.as_bytes());
    for part in parts {
        hasher.update([0u8]);
        hasher.update(part.as_bytes());
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(32);
    for byte in &digest[..16] {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub fn command_hash(salt: &[u8; 32], argv: &[String]) -> Option<String> {
    let rest: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();
    (!argv.is_empty()).then(|| evidence_hash(salt, "cmd", &rest))
}

pub fn output_hash(salt: &[u8; 32], path: &Path) -> String {
    evidence_hash(salt, "out", &[&super::find::canon(path).to_string_lossy()])
}

pub struct Evidence<'a> {
    pub salt: &'a [u8; 32],
    pub cmd_hash: Option<&'a str>,
    pub out_hash: Option<&'a str>,
    pub opened_ms: i64,
}
