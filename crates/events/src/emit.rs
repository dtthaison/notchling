use crate::event::Event;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Committed {
    pub seq: u64,
    pub audit_n: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnwritableWhy {
    Timeout,
    WriteFailed,
    Overflow,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unwritable {
    pub why: UnwritableWhy,
}

pub trait Emit: Send + Sync {
    fn push(&self, event: Event);
    fn commit(&self, event: Event) -> Result<Committed, Unwritable>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoEmit;

impl Emit for NoEmit {
    fn push(&self, _event: Event) {}

    fn commit(&self, _event: Event) -> Result<Committed, Unwritable> {
        Ok(Committed {
            seq: 0,
            audit_n: None,
        })
    }
}

#[derive(Debug, Default)]
pub struct MemorySink {
    events: Mutex<Vec<Event>>,
    failing: AtomicBool,
    commits: AtomicUsize,
}

impl MemorySink {
    pub fn new() -> MemorySink {
        MemorySink::default()
    }

    pub fn events(&self) -> Vec<Event> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.events().iter().map(|e| e.spec.name).collect()
    }

    pub fn fail_commits(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }

    pub fn commits(&self) -> usize {
        self.commits.load(Ordering::SeqCst)
    }

    fn record(&self, event: Event) -> u64 {
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        events.push(event);
        events.len() as u64
    }
}

impl Emit for MemorySink {
    fn push(&self, event: Event) {
        self.record(event);
    }

    fn commit(&self, event: Event) -> Result<Committed, Unwritable> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(Unwritable {
                why: UnwritableWhy::WriteFailed,
            });
        }
        let seq = self.record(event);
        self.commits.fetch_add(1, Ordering::SeqCst);
        Ok(Committed { seq, audit_n: None })
    }
}
