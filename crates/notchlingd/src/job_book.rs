use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::vault_caller::{Processes, SystemProcesses};
use notchling_engine::delegate::record::pid_alive;

pub const KEEP_ROWS: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub session: String,
    pub organization: String,
    pub kind: String,
    pub tool: String,
    pub folder: String,
    pub started_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i64>,
}

#[derive(Default, Serialize, Deserialize)]
struct Book {
    jobs: BTreeMap<String, Record>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pids: BTreeMap<String, u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    starts: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    lost: BTreeSet<String>,
}

pub struct JobBook {
    file: Option<PathBuf>,
    book: Mutex<Book>,
    counter: AtomicU64,
    procs: Arc<dyn Processes>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl JobBook {
    pub fn in_memory() -> JobBook {
        JobBook {
            file: None,
            book: Mutex::new(Book::default()),
            counter: AtomicU64::new(0),
            procs: Arc::new(SystemProcesses),
        }
    }

    pub fn with_processes(mut self, procs: Arc<dyn Processes>) -> JobBook {
        self.procs = procs;
        self
    }

    pub fn open(file: PathBuf) -> JobBook {
        let book = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Book>(&bytes).ok())
            .unwrap_or_default();
        JobBook {
            file: Some(file),
            book: Mutex::new(book),
            counter: AtomicU64::new(0),
            procs: Arc::new(SystemProcesses),
        }
    }

    fn save(&self, book: &Book) {
        let Some(file) = &self.file else {
            return;
        };
        let Ok(bytes) = serde_json::to_vec(book) else {
            return;
        };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = file.with_extension("tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, file);
        }
    }

    fn trim(book: &mut Book, kind: &str) {
        while book.jobs.values().filter(|r| r.kind == kind).count() > KEEP_ROWS {
            let oldest = book
                .jobs
                .iter()
                .filter(|(_, r)| r.kind == kind)
                .min_by_key(|(_, r)| (r.ended_ms.is_none(), r.started_ms))
                .map(|(k, _)| k.clone());
            match oldest {
                Some(key) => {
                    book.pids.remove(&key);
                    book.starts.remove(&key);
                    book.lost.remove(&key);
                    book.jobs.remove(&key)
                }
                None => break,
            };
        }
    }

    pub fn record(&self, id: &str, record: Record) {
        let mut book = lock(&self.book);
        let kind = record.kind.clone();
        book.jobs.insert(id.to_string(), record);
        Self::trim(&mut book, &kind);
        self.save(&book);
    }

    pub fn record_with_pid(&self, id: &str, record: Record, pid: Option<u32>) {
        let start = pid.and_then(|pid| self.procs.start_time(pid));
        let mut book = lock(&self.book);
        let kind = record.kind.clone();
        book.jobs.insert(id.to_string(), record);
        match pid {
            Some(pid) => book.pids.insert(id.to_string(), pid),
            None => book.pids.remove(id),
        };
        match start {
            Some(start) => book.starts.insert(id.to_string(), start),
            None => book.starts.remove(id),
        };
        book.lost.remove(id);
        Self::trim(&mut book, &kind);
        self.save(&book);
    }

    pub fn claim(&self, id: &str, record: Record) -> bool {
        let mut book = lock(&self.book);
        if let Some(have) = book.jobs.get(id) {
            return have.session == record.session;
        }
        let kind = record.kind.clone();
        book.jobs.insert(id.to_string(), record);
        Self::trim(&mut book, &kind);
        self.save(&book);
        true
    }

    pub fn get(&self, id: &str) -> Option<Record> {
        lock(&self.book).jobs.get(id).cloned()
    }

    pub fn owned_by(&self, session: &str, id: &str) -> Option<Record> {
        self.get(id).filter(|r| r.session == session)
    }

    pub fn mine(&self, session: &str) -> Vec<(String, Record)> {
        lock(&self.book)
            .jobs
            .iter()
            .filter(|(_, r)| r.session == session)
            .map(|(id, r)| (id.clone(), r.clone()))
            .collect()
    }

    pub fn all(&self) -> Vec<(String, Record)> {
        lock(&self.book)
            .jobs
            .iter()
            .map(|(id, r)| (id.clone(), r.clone()))
            .collect()
    }

    pub fn end(&self, id: &str, at_ms: i64, exit: Option<i64>) -> bool {
        let mut book = lock(&self.book);
        let lost = book.lost.contains(id);
        let Some(record) = book
            .jobs
            .get_mut(id)
            .filter(|r| r.ended_ms.is_none() || lost)
        else {
            return false;
        };
        record.ended_ms = Some(at_ms);
        record.exit = exit;
        book.pids.remove(id);
        book.starts.remove(id);
        book.lost.remove(id);
        self.save(&book);
        true
    }

    pub fn end_lost(&self, id: &str, at_ms: i64) -> bool {
        let mut book = lock(&self.book);
        if !Self::lose(&mut book, id, at_ms) {
            return false;
        }
        self.save(&book);
        true
    }

    fn lose(book: &mut Book, id: &str, at_ms: i64) -> bool {
        let Some(record) = book.jobs.get_mut(id).filter(|r| r.ended_ms.is_none()) else {
            return false;
        };
        record.ended_ms = Some(at_ms);
        record.exit = None;
        book.pids.remove(id);
        book.starts.remove(id);
        book.lost.insert(id.to_string());
        true
    }

    fn gone(&self, book: &Book, id: &str, record: &Record, now_ms: i64, stale_ms: i64) -> bool {
        let old = now_ms.saturating_sub(record.started_ms) > stale_ms;
        match (book.pids.get(id), book.starts.get(id)) {
            (Some(pid), _) if self.procs.exited(*pid) => true,
            (Some(pid), Some(start)) => self.procs.start_time(*pid) != Some(*start),
            (Some(pid), None) => old || !pid_alive(*pid),
            (None, _) => old,
        }
    }

    pub fn reap(&self, kind: &str, now_ms: i64, stale_ms: i64) -> usize {
        let mut book = lock(&self.book);
        let ids: Vec<String> = book
            .jobs
            .iter()
            .filter(|(_, r)| r.kind == kind && r.ended_ms.is_none())
            .filter(|(id, r)| self.gone(&book, id, r, now_ms, stale_ms))
            .map(|(id, _)| id.clone())
            .collect();
        let ended = ids
            .iter()
            .filter(|id| Self::lose(&mut book, id, now_ms))
            .count();
        if ended > 0 {
            self.save(&book);
        }
        ended
    }

    pub fn is_lost(&self, id: &str) -> bool {
        lock(&self.book).lost.contains(id)
    }

    pub fn unfinished(&self, kind: &str) -> Vec<(String, Record, Option<u32>)> {
        let book = lock(&self.book);
        book.jobs
            .iter()
            .filter(|(_, r)| r.kind == kind && r.ended_ms.is_none())
            .map(|(id, r)| (id.clone(), r.clone(), book.pids.get(id).copied()))
            .collect()
    }

    pub fn fresh_id(&self, now_ms: i64) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("d-{:x}{:04x}", now_ms.max(0), n & 0xffff)
    }
}

#[cfg(test)]
#[path = "job_book_lost_tests.rs"]
mod lost_tests;
