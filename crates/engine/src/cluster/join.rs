use std::collections::BTreeMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use super::id::NodeId;
use super::invite::Invite;

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

#[derive(Clone, Default)]
pub struct Tokens {
    issued: BTreeMap<String, Issued>,
}

#[derive(Clone)]
struct Issued {
    at: i64,
    minter: NodeId,
    invite: Option<Invite>,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("open", &self.issued.len())
            .finish()
    }
}

impl Tokens {
    pub fn issue(&mut self, secret: String, minter: NodeId, home: &NodeId, now_ms: i64) {
        self.issue_with(secret, minter, home, now_ms, None);
    }

    pub fn issue_invite(
        &mut self,
        secret: String,
        minter: NodeId,
        home: &NodeId,
        now_ms: i64,
        invite: Invite,
    ) {
        self.issue_with(secret, minter, home, now_ms, Some(invite));
    }

    fn issue_with(
        &mut self,
        secret: String,
        minter: NodeId,
        home: &NodeId,
        now_ms: i64,
        invite: Option<Invite>,
    ) {
        self.issued.retain(|_, t| now_ms - t.at <= TOKEN_TTL_MS);
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
        self.issued.insert(
            secret,
            Issued {
                at: now_ms,
                minter,
                invite,
            },
        );
    }

    fn count_of(&self, minter: &NodeId) -> usize {
        self.issued.values().filter(|t| t.minter == *minter).count()
    }

    fn drop_oldest(&mut self, wanted: impl Fn(&NodeId) -> bool) -> bool {
        let oldest = self
            .issued
            .iter()
            .filter(|(_, t)| wanted(&t.minter))
            .min_by(|a, b| a.1.at.cmp(&b.1.at).then_with(|| a.0.cmp(b.0)))
            .map(|(secret, _)| secret.clone());
        match oldest {
            Some(oldest) => self.issued.remove(&oldest).is_some(),
            None => false,
        }
    }

    pub fn open_invites(&self, now_ms: i64) -> Vec<(NodeId, String, i64)> {
        let mut open: Vec<(i64, NodeId, String)> = self
            .issued
            .values()
            .filter(|t| now_ms - t.at <= TOKEN_TTL_MS)
            .filter_map(|t| {
                let invite = t.invite.as_ref()?;
                Some((t.at, invite.node.clone(), invite.name.clone()))
            })
            .collect();
        open.sort();
        open.into_iter()
            .map(|(at, node, name)| (node, name, at + TOKEN_TTL_MS))
            .collect()
    }

    pub fn has_open(&self, now_ms: i64) -> bool {
        self.issued.values().any(|t| now_ms - t.at <= TOKEN_TTL_MS)
    }

    pub fn take(&mut self, secret: &str, now_ms: i64) -> Option<NodeId> {
        self.take_open(secret, now_ms).map(|(minter, _)| minter)
    }

    pub fn take_open(&mut self, secret: &str, now_ms: i64) -> Option<(NodeId, Option<Invite>)> {
        let t = self.issued.remove(secret)?;
        (now_ms - t.at <= TOKEN_TTL_MS).then_some((t.minter, t.invite))
    }
}
