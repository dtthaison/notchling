use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HELD_STALE_MS: i64 = 3_600_000;
pub const MAX_DELIVERED: usize = 256;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HeldTarget {
    Primary,
    Session(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Held {
    pub id: String,
    pub op: String,
    pub args: Value,
    pub target: HeldTarget,
    pub device: String,
    pub request: String,
    pub at_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Delivered {
    pub id: String,
    pub device: String,
    pub request: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeldQueue {
    pub(super) entries: Vec<Held>,
    pub(super) confirmed: Vec<String>,
    pub(super) delivered: Vec<Delivered>,
}

fn stale(h: &Held, now_ms: i64) -> bool {
    now_ms.saturating_sub(h.at_ms) > HELD_STALE_MS
}

impl HeldQueue {
    pub fn hold(&mut self, h: Held) -> String {
        if let Some(same) = self
            .entries
            .iter()
            .find(|e| e.device == h.device && e.request == h.request)
        {
            return same.id.clone();
        }
        if let Some(done) = self
            .delivered
            .iter()
            .find(|d| d.device == h.device && d.request == h.request)
        {
            return done.id.clone();
        }
        let id = h.id.clone();
        self.entries.push(h);
        id
    }

    pub fn cancel(&mut self, id: &str) -> bool {
        let Some(at) = self.entries.iter().position(|e| e.id == id) else {
            return false;
        };
        self.entries.remove(at);
        self.confirmed.retain(|c| c != id);
        true
    }

    pub fn was_delivered(&self, id: &str) -> bool {
        self.delivered.iter().any(|d| d.id == id)
    }

    pub fn list(&self) -> Vec<Held> {
        self.entries.clone()
    }

    pub fn take_all(&mut self) -> Vec<Held> {
        let all = std::mem::take(&mut self.entries);
        self.confirmed.clear();
        self.record(&all);
        all
    }

    pub fn take_due(&mut self, now_ms: i64) -> Vec<Held> {
        let mut due = Vec::new();
        let mut confirmed: Vec<(usize, Held)> = Vec::new();
        let mut kept = Vec::new();
        for entry in std::mem::take(&mut self.entries) {
            if !stale(&entry, now_ms) {
                due.push(entry);
            } else if let Some(rank) = self.confirmed.iter().position(|c| *c == entry.id) {
                confirmed.push((rank, entry));
            } else {
                kept.push(entry);
            }
        }
        confirmed.sort_by_key(|(rank, _)| *rank);
        due.extend(confirmed.into_iter().map(|(_, entry)| entry));
        self.entries = kept;
        let entries = &self.entries;
        self.confirmed
            .retain(|c| entries.iter().any(|e| e.id == *c));
        self.record(&due);
        due
    }

    pub fn waiting(&self, now_ms: i64) -> Vec<Held> {
        self.entries
            .iter()
            .filter(|e| stale(e, now_ms) && !self.confirmed.contains(&e.id))
            .cloned()
            .collect()
    }

    pub fn confirm(&mut self, id: &str) -> bool {
        if !self.entries.iter().any(|e| e.id == id) {
            return false;
        }
        if !self.confirmed.iter().any(|c| c == id) {
            self.confirmed.push(id.to_string());
        }
        true
    }

    fn record(&mut self, sent: &[Held]) {
        self.delivered.extend(sent.iter().map(|h| Delivered {
            id: h.id.clone(),
            device: h.device.clone(),
            request: h.request.clone(),
        }));
        let over = self.delivered.len().saturating_sub(MAX_DELIVERED);
        self.delivered.drain(..over);
    }
}
