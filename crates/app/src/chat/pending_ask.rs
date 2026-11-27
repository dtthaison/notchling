use super::data::Session;
use super::sidebar::Pending;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingAsk {
    pub key: String,
    expires_ms: i64,
}

pub fn row_for_key(sessions: &[Session], key: &str) -> Option<String> {
    sessions
        .iter()
        .find(|s| s.agent.outbox_key() == key)
        .map(|s| s.agent.id.clone())
}

impl PendingAsk {
    pub const WAIT_MS: i64 = 5_000;

    pub fn new(key: &str, now_ms: i64) -> PendingAsk {
        PendingAsk {
            key: key.to_owned(),
            expires_ms: now_ms.saturating_add(Self::WAIT_MS),
        }
    }

    pub fn after_load(&self, sessions: &[Session], now_ms: i64) -> Pending {
        match row_for_key(sessions, &self.key) {
            Some(id) => Pending::Found(id),
            None if now_ms >= self.expires_ms => Pending::GaveUp,
            None => Pending::Waiting,
        }
    }
}

#[cfg(test)]
#[path = "pending_ask_tests.rs"]
mod tests;
