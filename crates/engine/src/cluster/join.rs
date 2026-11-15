use std::collections::BTreeMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use super::id::NodeId;

pub const TOKEN_TTL_MS: i64 = 600_000;
pub const MAX_OPEN_TOKENS: usize = 16;
pub const MAX_TOKENS_PER_MINTER: usize = 4;
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
    issued: BTreeMap<String, (i64, NodeId)>,
}

impl Tokens {
    pub fn issue(&mut self, secret: String, minter: NodeId, home: &NodeId, now_ms: i64) {
        self.issued
            .retain(|_, (at, _)| now_ms - *at <= TOKEN_TTL_MS);
        self.issued.remove(&secret);
        while self.count_of(&minter) >= MAX_TOKENS_PER_MINTER {
            if !self.drop_oldest(|by| *by == minter) {
                break;
            }
        }
        while self.issued.len() >= MAX_OPEN_TOKENS {
            if !self.drop_oldest(|by| by != home) && !self.drop_oldest(|_| true) {
                break;
            }
        }
        self.issued.insert(secret, (now_ms, minter));
    }

    fn count_of(&self, minter: &NodeId) -> usize {
        self.issued.values().filter(|(_, by)| by == minter).count()
    }

    fn drop_oldest(&mut self, wanted: impl Fn(&NodeId) -> bool) -> bool {
        let oldest = self
            .issued
            .iter()
            .filter(|(_, (_, by))| wanted(by))
            .min_by(|a, b| a.1 .0.cmp(&b.1 .0).then_with(|| a.0.cmp(b.0)))
            .map(|(secret, _)| secret.clone());
        match oldest {
            Some(oldest) => self.issued.remove(&oldest).is_some(),
            None => false,
        }
    }

    pub fn has_open(&self, now_ms: i64) -> bool {
        self.issued
            .values()
            .any(|(at, _)| now_ms - *at <= TOKEN_TTL_MS)
    }

    pub fn take(&mut self, secret: &str, now_ms: i64) -> Option<NodeId> {
        let (at, minter) = self.issued.remove(secret)?;
        (now_ms - at <= TOKEN_TTL_MS).then_some(minter)
    }
}
