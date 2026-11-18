use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use notchling_engine::machines::device_login::DevicePrompt;
use notchling_engine::machines::step::FixOutcome;
use notchling_engine::versions::{Tool, Version};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Run {
    Running(String),
    Done { ok: bool, text: String },
}

type Installed = (i64, Option<Version>);

#[derive(Clone)]
pub struct Session {
    pub tool: Tool,
    pub prompt: Option<DevicePrompt>,
    pub cancel: Arc<AtomicBool>,
}

pub struct ToolRuns {
    entries: Mutex<BTreeMap<String, Run>>,
    sessions: Mutex<BTreeMap<String, Session>>,
    marks: Mutex<BTreeMap<(String, Tool), i64>>,
    installed: Mutex<BTreeMap<(String, Tool), Installed>>,
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
            sessions: Mutex::new(BTreeMap::new()),
            marks: Mutex::new(BTreeMap::new()),
            installed: Mutex::new(BTreeMap::new()),
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

impl ToolRuns {
    fn sessions(&self) -> MutexGuard<'_, BTreeMap<String, Session>> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn open_session(&self, machine: &str, tool: Tool) -> Option<Arc<AtomicBool>> {
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut sessions = self.sessions();
            if sessions.contains_key(machine) {
                return None;
            }
            sessions.insert(
                machine.to_owned(),
                Session {
                    tool,
                    prompt: None,
                    cancel: cancel.clone(),
                },
            );
        }
        self.changed();
        Some(cancel)
    }

    pub fn offer(&self, machine: &str, prompt: DevicePrompt) {
        {
            let mut sessions = self.sessions();
            let Some(session) = sessions.get_mut(machine) else {
                return;
            };
            session.prompt = Some(prompt);
        }
        self.changed();
    }

    pub fn session(&self, machine: &str) -> Option<Session> {
        self.sessions().get(machine).cloned()
    }

    pub fn cancel(&self, machine: &str) {
        if let Some(session) = self.sessions().get(machine) {
            session.cancel.store(true, Ordering::SeqCst);
        }
        self.changed();
    }

    pub fn close_session(&self, machine: &str) {
        let removed = self.sessions().remove(machine).is_some();
        if removed {
            self.changed();
        }
    }

    pub fn mark_signed_in(&self, machine: &str, tool: Tool, now_ms: i64) {
        self.marks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((machine.to_owned(), tool), now_ms);
        self.changed();
    }

    pub fn signed_in_since(&self, machine: &str, tool: Tool) -> Option<i64> {
        self.marks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(machine.to_owned(), tool))
            .copied()
    }

    pub fn mark_installed(&self, machine: &str, tool: Tool, version: Option<Version>, now_ms: i64) {
        self.installed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((machine.to_owned(), tool), (now_ms, version));
        self.changed();
    }

    pub fn installed_since(&self, machine: &str, tool: Tool) -> Option<(i64, Option<Version>)> {
        self.installed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(machine.to_owned(), tool))
            .cloned()
    }
}
