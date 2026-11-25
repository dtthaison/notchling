use crate::embed_fetch::{progress_json, REMOVING, STOPPING};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum After {
    Report,
    Again,
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Running,
    Finishing,
    Gone,
}

#[derive(Debug)]
pub(crate) enum Settle {
    Again(Arc<AtomicBool>),
    Done(After),
}

struct Job {
    phase: Phase,
    cancel: Arc<AtomicBool>,
    after: After,
    removers: usize,
    bytes: u64,
    total: u64,
}

impl Job {
    fn new(total: u64) -> Job {
        Job {
            phase: Phase::Running,
            cancel: Arc::new(AtomicBool::new(false)),
            after: After::Report,
            removers: 0,
            bytes: 0,
            total,
        }
    }

    fn stop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        if self.after == After::Again {
            self.after = After::Report;
        }
    }
}

#[derive(Default)]
pub(crate) struct Jobs {
    entries: HashMap<String, Job>,
}

impl Jobs {
    pub(crate) fn new() -> Jobs {
        Jobs::default()
    }

    pub(crate) fn admit(
        &mut self,
        id: &str,
        total: u64,
    ) -> Result<Option<Arc<AtomicBool>>, &'static str> {
        if let Some(job) = self.entries.get_mut(id) {
            if job.removers > 0 {
                return Err(REMOVING);
            }
            if job.phase != Phase::Running {
                return Err(STOPPING);
            }
            if job.cancel.load(Ordering::SeqCst) {
                job.after = After::Again;
            }
            return Ok(None);
        }
        let job = Job::new(total);
        let cancel = job.cancel.clone();
        self.entries.insert(id.to_string(), job);
        Ok(Some(cancel))
    }

    pub(crate) fn abort(&mut self, id: &str) {
        self.close(id);
    }

    pub(crate) fn settle(&mut self, id: &str, total: u64) -> Settle {
        let Some(job) = self.entries.get_mut(id) else {
            return Settle::Done(After::Nothing);
        };
        if job.after == After::Again {
            let next = Job::new(total);
            let cancel = next.cancel.clone();
            *job = next;
            return Settle::Again(cancel);
        }
        job.phase = Phase::Finishing;
        Settle::Done(job.after)
    }

    pub(crate) fn close(&mut self, id: &str) {
        let Some(job) = self.entries.get_mut(id) else {
            return;
        };
        if job.removers > 0 {
            job.phase = Phase::Gone;
        } else {
            self.entries.remove(id);
        }
    }

    pub(crate) fn register_remover(&mut self, id: &str) {
        if let Some(job) = self.entries.get_mut(id) {
            job.removers += 1;
            job.cancel.store(true, Ordering::SeqCst);
            if job.phase == Phase::Running {
                job.after = After::Nothing;
            }
        }
    }

    pub(crate) fn waiting(&self, id: &str) -> bool {
        matches!(self.entries.get(id), Some(job) if job.phase != Phase::Gone)
    }

    pub(crate) fn leave(&mut self, id: &str) -> bool {
        let Some(job) = self.entries.get_mut(id) else {
            return false;
        };
        job.removers = job.removers.saturating_sub(1);
        if job.removers > 0 {
            return false;
        }
        if job.phase == Phase::Gone {
            self.entries.remove(id);
            return true;
        }
        job.after = After::Report;
        false
    }

    pub(crate) fn cancel(&mut self, id: &str) -> bool {
        match self.entries.get_mut(id) {
            Some(job) if job.phase == Phase::Running => {
                job.stop();
                true
            }
            _ => false,
        }
    }

    pub(crate) fn cancel_all(&mut self) {
        for job in self.entries.values_mut() {
            if job.phase == Phase::Running {
                job.stop();
            }
        }
    }

    pub(crate) fn progress(&mut self, id: &str, bytes: u64) {
        if let Some(job) = self.entries.get_mut(id) {
            if job.phase == Phase::Running {
                job.bytes = bytes;
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn is_running(&self, id: &str) -> bool {
        matches!(self.entries.get(id), Some(j) if j.phase == Phase::Running)
    }

    pub(crate) fn list(&self, results: &HashMap<String, Value>) -> Vec<Value> {
        let mut rows: Vec<Value> = self
            .entries
            .iter()
            .filter(|(_, j)| j.phase == Phase::Running)
            .map(|(id, j)| progress_json(id, "downloading", j.bytes, j.total, None))
            .collect();
        for (id, result) in results {
            if !self.is_running(id) {
                rows.push(result.clone());
            }
        }
        rows.sort_by(|a, b| a["model"].as_str().cmp(&b["model"].as_str()));
        rows
    }
}
