use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HELD_STALE_MS: i64 = 3_600_000;
pub const MAX_DELIVERED: usize = 256;
pub const MAX_DELIVERED_PER_DEVICE: usize = 64;

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

#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    Held(Held),
    Delivered(String),
}

impl HeldQueue {
    pub fn find(&self, device: &str, request: &str) -> Option<Found> {
        if let Some(held) = self
            .entries
            .iter()
            .find(|e| e.device == device && e.request == request)
        {
            return Some(Found::Held(held.clone()));
        }
        self.delivered
            .iter()
            .find(|d| d.device == device && d.request == request)
            .map(|d| Found::Delivered(d.id.clone()))
    }

    pub fn note_sent(&mut self, id: &str, device: &str, request: &str) {
        self.delivered
            .retain(|d| !(d.device == device && d.request == request));
        self.delivered.push(Delivered {
            id: id.to_string(),
            device: device.to_string(),
            request: request.to_string(),
        });
        self.trim_delivered();
    }

    fn trim_delivered(&mut self) {
        let mut counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for done in &self.delivered {
            *counts.entry(done.device.clone()).or_default() += 1;
        }
        self.delivered.retain(|done| {
            let count = counts.entry(done.device.clone()).or_default();
            if *count > MAX_DELIVERED_PER_DEVICE {
                *count -= 1;
                false
            } else {
                true
            }
        });
        let over = self.delivered.len().saturating_sub(MAX_DELIVERED);
        self.delivered.drain(..over);
    }

    pub fn forget_sent(&mut self, device: &str, request: &str) {
        self.delivered
            .retain(|d| !(d.device == device && d.request == request));
    }

    pub fn owns(&self, id: &str, device: &str) -> bool {
        self.entries
            .iter()
            .any(|e| e.id == id && e.device == device)
    }

    pub fn delivered_to(&self, id: &str, device: &str) -> bool {
        self.delivered
            .iter()
            .any(|d| d.id == id && d.device == device)
    }

    pub fn count(&self, device: Option<&str>) -> usize {
        self.entries
            .iter()
            .filter(|e| device.is_none_or(|d| e.device == d))
            .count()
    }

    pub fn is_waiting(&self, id: &str, now_ms: i64) -> bool {
        self.entries
            .iter()
            .any(|e| e.id == id && stale(e, now_ms) && !self.confirmed.contains(&e.id))
    }

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
        self.take_due_except(now_ms, &std::collections::BTreeSet::new())
    }

    pub fn take_due_except(
        &mut self,
        now_ms: i64,
        resting: &std::collections::BTreeSet<String>,
    ) -> Vec<Held> {
        let mut due = Vec::new();
        let mut confirmed: Vec<(usize, Held)> = Vec::new();
        let mut kept = Vec::new();
        for entry in std::mem::take(&mut self.entries) {
            if resting.contains(&entry.id) {
                kept.push(entry);
            } else if !stale(&entry, now_ms) {
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

    pub fn keep_back(&mut self, back: Vec<Held>, now_ms: i64) {
        for entry in &back {
            self.delivered.retain(|d| d.id != entry.id);
            if stale(entry, now_ms) && !self.confirmed.contains(&entry.id) {
                self.confirmed.push(entry.id.clone());
            }
        }
        let mut entries = back;
        entries.append(&mut self.entries);
        self.entries = entries;
    }

    pub fn absorb(&mut self, earlier: &HeldQueue) {
        for entry in &earlier.entries {
            if self.entries.iter().any(|e| e.id == entry.id) {
                continue;
            }
            self.entries.push(entry.clone());
            if earlier.confirmed.contains(&entry.id) {
                self.confirmed.push(entry.id.clone());
            }
        }
        for done in &earlier.delivered {
            if !self.delivered.iter().any(|d| d.id == done.id) {
                self.delivered.push(done.clone());
            }
        }
        self.trim_delivered();
    }

    fn record(&mut self, sent: &[Held]) {
        self.delivered.extend(sent.iter().map(|h| Delivered {
            id: h.id.clone(),
            device: h.device.clone(),
            request: h.request.clone(),
        }));
        self.trim_delivered();
    }
}
