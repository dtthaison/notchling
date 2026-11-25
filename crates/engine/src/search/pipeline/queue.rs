use crate::search::embed::ModelId;
use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

pub const BATCH: usize = 8;
pub const WAKE_AT: usize = 16;
pub const WAKE_AFTER: f64 = 300.0;
pub const IDLE_EXIT: f64 = 300.0;

struct Entry {
    model: ModelId,
    hash: [u8; 16],
    text: String,
    at: f64,
}

#[derive(Default)]
struct Inner {
    pending: VecDeque<Entry>,
    held: HashSet<(ModelId, [u8; 16])>,
    backlog: bool,
}

#[derive(Default)]
pub struct Queue {
    inner: Mutex<Inner>,
}

impl Queue {
    pub fn new() -> Queue {
        Queue::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn push(&self, model: &ModelId, rows: Vec<([u8; 16], String)>, now: f64) -> usize {
        let mut inner = self.lock();
        let mut added = 0;
        for (hash, text) in rows {
            if !inner.held.insert((model.clone(), hash)) {
                continue;
            }
            inner.pending.push_back(Entry {
                model: model.clone(),
                hash,
                text,
                at: now,
            });
            added += 1;
        }
        added
    }

    pub fn backlog(&self) -> bool {
        self.lock().backlog
    }

    pub fn set_backlog(&self, on: bool) {
        self.lock().backlog = on;
    }

    pub fn due(&self, now: f64) -> bool {
        let inner = self.lock();
        let Some(oldest) = inner.pending.front() else {
            return false;
        };
        inner.backlog || inner.pending.len() >= WAKE_AT || now - oldest.at >= WAKE_AFTER
    }

    pub fn next_model(&self) -> Option<ModelId> {
        self.lock().pending.front().map(|e| e.model.clone())
    }

    pub fn take(&self, n: usize) -> Vec<([u8; 16], String)> {
        let mut inner = self.lock();
        let Some(model) = inner.pending.front().map(|e| e.model.clone()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut rest = VecDeque::with_capacity(inner.pending.len());
        while let Some(entry) = inner.pending.pop_front() {
            if out.len() < n && entry.model == model {
                out.push((entry.hash, entry.text));
            } else {
                rest.push_back(entry);
            }
        }
        inner.pending = rest;
        out
    }

    pub fn release(&self, model: &ModelId, hashes: &[[u8; 16]]) {
        let mut inner = self.lock();
        for hash in hashes {
            inner.held.remove(&(model.clone(), *hash));
        }
    }

    pub fn pending_hashes(&self) -> Vec<[u8; 16]> {
        let seen: HashSet<[u8; 16]> = self.lock().pending.iter().map(|e| e.hash).collect();
        seen.into_iter().collect()
    }

    pub fn prune(&self, live: &HashSet<[u8; 16]>) -> usize {
        let mut inner = self.lock();
        let mut removed = 0;
        let mut kept = VecDeque::with_capacity(inner.pending.len());
        while let Some(entry) = inner.pending.pop_front() {
            if live.contains(&entry.hash) {
                kept.push_back(entry);
            } else {
                inner.held.remove(&(entry.model, entry.hash));
                removed += 1;
            }
        }
        inner.pending = kept;
        removed
    }

    pub fn held(&self, model: &ModelId) -> usize {
        self.lock().held.iter().filter(|(m, _)| m == model).count()
    }

    pub fn drop_model(&self, model: &ModelId) {
        let mut inner = self.lock();
        inner.pending.retain(|e| e.model != *model);
        inner.held.retain(|(m, _)| m != model);
    }

    pub fn len(&self) -> usize {
        self.lock().pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
