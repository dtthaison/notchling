use crate::sessions::Agent;
use std::collections::{BTreeMap, BTreeSet};

pub const START_GRACE_MS: i64 = 20_000;
pub const SETTLE_MS: i64 = 3_000;
pub const BLOCKED_RETRY_MS: i64 = 30_000;
pub const JOIN: &str = "\n\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tried {
    pub len: usize,
    pub at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Send,
    Hold(String),
}

#[derive(Clone, Debug, Default)]
pub struct Held {
    texts: BTreeMap<String, String>,
    sent_at: BTreeMap<String, i64>,
    calm_since: BTreeMap<String, i64>,
    tried: BTreeMap<String, Tried>,
    notes: BTreeMap<String, String>,
    flying: BTreeMap<String, i64>,
    discarded: BTreeSet<String>,
}

impl Held {
    pub fn route(&mut self, id: &str, text: &str, working: bool, now_ms: i64) -> Route {
        if working || self.texts.contains_key(id) || self.starting(id, now_ms) {
            let held = self.texts.entry(id.to_owned()).or_default();
            if held.is_empty() {
                held.push_str(text);
            } else {
                held.truncate(held.trim_end().len());
                held.push_str(JOIN);
                held.push_str(text.trim_start_matches(['\n', '\r']));
            }
            return Route::Hold(held.clone());
        }
        self.sent_at.insert(id.to_owned(), now_ms);
        Route::Send
    }

    pub fn not_sent(&mut self, id: &str) {
        self.sent_at.remove(id);
    }

    pub fn waiting(&self) -> Vec<String> {
        self.texts.keys().cloned().collect()
    }

    pub fn ready(
        &mut self,
        rows: &[Agent],
        now_ms: i64,
        calm: &BTreeSet<String>,
    ) -> Vec<(String, String)> {
        for row in rows {
            if row.turn_running() {
                self.sent_at.remove(&row.id);
            }
        }
        self.sent_at
            .retain(|_, at| now_ms.saturating_sub(*at) < START_GRACE_MS);
        let texts = &self.texts;
        self.calm_since
            .retain(|id, _| calm.contains(id) && texts.contains_key(id));
        for id in calm.iter().filter(|id| texts.contains_key(*id)) {
            self.calm_since.entry(id.clone()).or_insert(now_ms);
        }
        let free: Vec<String> = rows
            .iter()
            .filter(|row| row.takes_held_text())
            .filter(|row| self.texts.contains_key(&row.id) && !self.sent_at.contains_key(&row.id))
            .filter(|row| {
                self.calm_since
                    .get(&row.id)
                    .is_some_and(|since| now_ms.saturating_sub(*since) >= SETTLE_MS)
            })
            .map(|row| row.id.clone())
            .collect();
        free.into_iter()
            .filter_map(|id| {
                let text = self.texts.remove(&id)?;
                self.calm_since.remove(&id);
                self.sent_at.insert(id.clone(), now_ms);
                self.flying.insert(id.clone(), now_ms);
                Some((id, text))
            })
            .collect()
    }

    pub fn put_back(&mut self, id: &str, text: &str, now_ms: i64) {
        self.sent_at.insert(id.to_owned(), now_ms);
        self.calm_since.remove(id);
        let held = self.texts.entry(id.to_owned()).or_default();
        *held = if held.is_empty() {
            text.to_owned()
        } else {
            format!("{text}{JOIN}{held}")
        };
    }

    pub fn failed(
        &mut self,
        id: &str,
        text: &str,
        now_ms: i64,
        typed: bool,
        note: Option<&str>,
    ) -> bool {
        let began = self.flying.remove(id).unwrap_or(now_ms);
        if self.discarded.remove(id) {
            return false;
        }
        self.put_back(id, text, now_ms);
        if note.is_some() {
            self.sent_at
                .insert(id.to_owned(), now_ms + (BLOCKED_RETRY_MS - START_GRACE_MS));
        }
        if typed {
            self.tried.insert(
                id.to_owned(),
                Tried {
                    len: text.len(),
                    at: began,
                },
            );
        }
        match note {
            Some(note) => self.notes.insert(id.to_owned(), note.to_owned()),
            None => self.notes.remove(id),
        };
        true
    }

    pub fn forget_tried(&mut self, id: &str) {
        self.tried.remove(id);
        self.notes.remove(id);
    }

    pub fn settled_ok(&mut self, id: &str) {
        self.flying.remove(id);
        self.discarded.remove(id);
        self.tried.remove(id);
        self.notes.remove(id);
    }

    pub fn tried(&self, id: &str) -> Option<Tried> {
        self.tried.get(id).copied()
    }

    pub fn note(&self, id: &str) -> Option<&str> {
        self.notes.get(id).map(String::as_str)
    }

    pub fn discard(&mut self, id: &str) -> bool {
        let was_held = self.texts.remove(id).is_some();
        let in_flight = self.flying.contains_key(id);
        if in_flight {
            self.discarded.insert(id.to_owned());
        }
        self.sent_at.remove(id);
        self.calm_since.remove(id);
        self.tried.remove(id);
        self.notes.remove(id);
        was_held || in_flight
    }

    pub fn get(&self, id: &str) -> Option<&str> {
        self.texts.get(id).map(String::as_str)
    }

    pub fn all(&self) -> &BTreeMap<String, String> {
        &self.texts
    }

    fn starting(&self, id: &str, now_ms: i64) -> bool {
        self.sent_at
            .get(id)
            .is_some_and(|at| now_ms.saturating_sub(*at) < START_GRACE_MS)
    }
}

pub fn after_tried(text: &str, tried: Tried) -> &str {
    text.get(tried.len..)
        .unwrap_or("")
        .trim_start_matches(['\n', '\r'])
}

#[cfg(test)]
#[path = "held_tests.rs"]
mod tests;
