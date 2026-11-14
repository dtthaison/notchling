use std::collections::BTreeMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use super::id::NodeId;

pub const TOKEN_TTL_MS: i64 = 600_000;
const PREFIX: &str = "nlj1.";
const MAX_TOKEN: usize = 4096;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct JoinToken {
    pub cluster: String,
    pub home: NodeId,
    pub fingerprint: String,
    pub addrs: Vec<String>,
    pub port: u16,
    pub secret: String,
}

pub fn encode_token(t: &JoinToken) -> String {
    let json = serde_json::to_vec(t).unwrap_or_default();
    format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
}

pub fn decode_token(s: &str) -> Result<JoinToken, String> {
    if s.len() > MAX_TOKEN {
        return Err("join token is too long".to_string());
    }
    let body = s
        .trim()
        .strip_prefix(PREFIX)
        .ok_or_else(|| "not a join token".to_string())?;
    let bytes = URL_SAFE_NO_PAD
        .decode(body)
        .map_err(|_| "not a join token".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "not a join token".to_string())
}

#[derive(Clone, Debug, Default)]
pub struct Tokens {
    issued: BTreeMap<String, i64>,
}

impl Tokens {
    pub fn issue(&mut self, secret: String, now_ms: i64) {
        self.issued.retain(|_, at| now_ms - *at <= TOKEN_TTL_MS);
        self.issued.insert(secret, now_ms);
    }

    pub fn has_open(&self, now_ms: i64) -> bool {
        self.issued.values().any(|at| now_ms - *at <= TOKEN_TTL_MS)
    }

    pub fn take(&mut self, secret: &str, now_ms: i64) -> bool {
        match self.issued.remove(secret) {
            Some(at) => now_ms - at <= TOKEN_TTL_MS,
            None => false,
        }
    }
}
