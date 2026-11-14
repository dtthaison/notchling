use crate::sessions::Agent;
use std::collections::BTreeMap;

pub const START_GRACE_MS: i64 = 20_000;
pub const JOIN: &str = "\n\n";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Send,
    Hold(String),
}

#[derive(Clone, Debug, Default)]
pub struct Held {
    texts: BTreeMap<String, String>,
    sent_at: BTreeMap<String, i64>,
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

    pub fn ready(&mut self, rows: &[Agent], now_ms: i64) -> Vec<(String, String)> {
        for row in rows {
            if row.turn_running() {
                self.sent_at.remove(&row.id);
            }
        }
        self.sent_at
            .retain(|_, at| now_ms.saturating_sub(*at) < START_GRACE_MS);
        let free: Vec<String> = rows
            .iter()
            .filter(|row| row.takes_held_text())
            .filter(|row| self.texts.contains_key(&row.id) && !self.sent_at.contains_key(&row.id))
            .map(|row| row.id.clone())
            .collect();
        free.into_iter()
            .filter_map(|id| {
                let text = self.texts.remove(&id)?;
                self.sent_at.insert(id.clone(), now_ms);
                Some((id, text))
            })
            .collect()
    }

    pub fn put_back(&mut self, id: &str, text: &str, now_ms: i64) {
        self.sent_at.insert(id.to_owned(), now_ms);
        let held = self.texts.entry(id.to_owned()).or_default();
        *held = if held.is_empty() {
            text.to_owned()
        } else {
            format!("{text}{JOIN}{held}")
        };
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

#[cfg(test)]
#[path = "held_tests.rs"]
mod tests;
