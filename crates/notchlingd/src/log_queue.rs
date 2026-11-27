use notchling_events::{Committed, Envelope, Event, Unwritable, UnwritableWhy};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Stamped {
    pub env: Envelope,
    pub event: Event,
    pub commit: Option<CommitTicket>,
}

#[derive(Debug)]
struct TicketInner {
    answer: Mutex<Option<Result<Committed, Unwritable>>>,
    ready: Condvar,
}

#[derive(Clone, Debug)]
pub struct CommitTicket(Arc<TicketInner>);

impl CommitTicket {
    pub fn new() -> CommitTicket {
        CommitTicket(Arc::new(TicketInner {
            answer: Mutex::new(None),
            ready: Condvar::new(),
        }))
    }

    pub fn answer(&self, result: Result<Committed, Unwritable>) {
        let mut slot = self.0.answer.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            *slot = Some(result);
        }
        self.0.ready.notify_all();
    }

    pub fn wait(&self, within: Duration) -> Result<Committed, Unwritable> {
        let slot = self.0.answer.lock().unwrap_or_else(PoisonError::into_inner);
        let (slot, _) = self
            .0
            .ready
            .wait_timeout_while(slot, within, |answer| answer.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        (*slot).unwrap_or(Err(Unwritable {
            why: UnwritableWhy::Timeout,
        }))
    }
}

impl Default for CommitTicket {
    fn default() -> CommitTicket {
        CommitTicket::new()
    }
}

#[derive(Default)]
pub struct Queues {
    pub always: VecDeque<Stamped>,
    pub toggle: VecDeque<Stamped>,
    pub always_bytes: usize,
    pub closed: bool,
}

impl Queues {
    pub fn is_empty(&self) -> bool {
        self.always.is_empty() && self.toggle.is_empty()
    }

    pub fn has_commit(&self) -> bool {
        self.always.iter().any(|s| s.commit.is_some())
    }

    pub fn len(&self) -> usize {
        self.always.len() + self.toggle.len()
    }

    pub fn take_batch(&mut self, max: usize) -> Vec<Stamped> {
        let mut batch = Vec::new();
        while batch.len() < max {
            let Some(next) = self.always.pop_front() else {
                break;
            };
            self.always_bytes = self.always_bytes.saturating_sub(next.event.json_len());
            let committed = next.commit.is_some();
            batch.push(next);
            if committed {
                return batch;
            }
        }
        while batch.len() < max {
            match self.toggle.pop_front() {
                Some(next) => batch.push(next),
                None => break,
            }
        }
        batch
    }
}
