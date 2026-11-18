use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use notchling_engine::machines::step::FixOutcome;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Run {
    Running(String),
    Done { ok: bool, text: String },
}

pub struct ToolRuns {
    entries: Mutex<BTreeMap<String, Run>>,
    revision: AtomicU64,
    generation: Arc<AtomicU64>,
}

impl Default for ToolRuns {
    fn default() -> ToolRuns {
        ToolRuns::tied_to(Arc::new(AtomicU64::new(0)))
    }
}

impl ToolRuns {
    pub fn tied_to(generation: Arc<AtomicU64>) -> ToolRuns {
        ToolRuns {
            entries: Mutex::new(BTreeMap::new()),
            revision: AtomicU64::new(0),
            generation,
        }
    }

    fn entries(&self) -> MutexGuard<'_, BTreeMap<String, Run>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn changed(&self) {
        self.revision.fetch_add(1, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    pub fn begin(&self, machine: &str, text: &str) -> bool {
        {
            let mut entries = self.entries();
            if matches!(entries.get(machine), Some(Run::Running(_))) {
                return false;
            }
            entries.insert(machine.to_owned(), Run::Running(text.to_owned()));
        }
        self.changed();
        true
    }

    pub fn say(&self, machine: &str, text: &str) {
        {
            let mut entries = self.entries();
            if !matches!(entries.get(machine), Some(Run::Running(_))) {
                return;
            }
            entries.insert(machine.to_owned(), Run::Running(text.to_owned()));
        }
        self.changed();
    }

    pub fn finish(&self, machine: &str, outcome: &FixOutcome) {
        self.entries().insert(
            machine.to_owned(),
            Run::Done {
                ok: outcome.ok,
                text: outcome.message.clone(),
            },
        );
        self.changed();
    }

    pub fn dismiss(&self, machine: &str) {
        let removed = {
            let mut entries = self.entries();
            if matches!(entries.get(machine), Some(Run::Done { .. })) {
                entries.remove(machine);
                true
            } else {
                false
            }
        };
        if removed {
            self.changed();
        }
    }

    pub fn get(&self, machine: &str) -> Option<Run> {
        self.entries().get(machine).cloned()
    }

    pub fn running(&self, machine: &str) -> bool {
        matches!(self.get(machine), Some(Run::Running(_)))
    }
}
