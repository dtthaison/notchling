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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sheet {
    pub machine: String,
    pub tool: Tool,
    pub node: String,
}

pub type Installed = (i64, Option<Version>);

#[derive(Clone)]
pub struct Session {
    pub tool: Tool,
    pub prompt: Option<DevicePrompt>,
    pub cancel: Arc<AtomicBool>,
}

pub struct ToolRuns {
    entries: Mutex<BTreeMap<String, Run>>,
    sessions: Mutex<BTreeMap<String, Session>>,
    sheet: Mutex<Option<Sheet>>,
    ids: Mutex<BTreeMap<String, u64>>,
    next_id: AtomicU64,
    logins: Mutex<BTreeMap<(String, Tool), (i64, bool)>>,
    installed: Mutex<BTreeMap<(String, Tool), Installed>>,
    revision: AtomicU64,
    generation: Arc<AtomicU64>,
}

impl Default for ToolRuns {
    fn default() -> ToolRuns {
        ToolRuns::tied_to(Arc::new(AtomicU64::new(0)))
    }
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl ToolRuns {
    pub fn tied_to(generation: Arc<AtomicU64>) -> ToolRuns {
        ToolRuns {
            entries: Mutex::new(BTreeMap::new()),
            sessions: Mutex::new(BTreeMap::new()),
            sheet: Mutex::new(None),
            ids: Mutex::new(BTreeMap::new()),
            next_id: AtomicU64::new(0),
            logins: Mutex::new(BTreeMap::new()),
            installed: Mutex::new(BTreeMap::new()),
            revision: AtomicU64::new(0),
            generation,
        }
    }

    fn entries(&self) -> MutexGuard<'_, BTreeMap<String, Run>> {
        locked(&self.entries)
    }

    fn changed(&self) {
        self.revision.fetch_add(1, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    pub fn begin(&self, machine: &str, text: &str) -> bool {
        self.begin_run(machine, text).is_some()
    }

    pub fn begin_run(&self, machine: &str, text: &str) -> Option<u64> {
        let id = {
            let mut entries = self.entries();
            if matches!(entries.get(machine), Some(Run::Running(_))) {
                return None;
            }
            entries.insert(machine.to_owned(), Run::Running(text.to_owned()));
            let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
            locked(&self.ids).insert(machine.to_owned(), id);
            id
        };
        self.changed();
        Some(id)
    }

    pub fn is_current(&self, machine: &str, id: u64) -> bool {
        locked(&self.ids).get(machine) == Some(&id)
    }

    pub fn finish_run(&self, machine: &str, id: u64, outcome: &FixOutcome) {
        if self.is_current(machine, id) {
            self.finish(machine, outcome);
        }
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
        locked(&self.sessions)
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

    pub fn offer(&self, machine: &str, owner: &Arc<AtomicBool>, prompt: DevicePrompt) {
        {
            let mut sessions = self.sessions();
            let Some(session) = sessions.get_mut(machine) else {
                return;
            };
            if !Arc::ptr_eq(&session.cancel, owner) {
                return;
            }
            session.prompt = Some(prompt);
        }
        self.changed();
    }

    pub fn session(&self, machine: &str) -> Option<Session> {
        self.sessions().get(machine).cloned()
    }

    pub fn cancel(&self, machine: &str, stopping: &str) {
        let session = self.sessions().remove(machine);
        let Some(session) = session else {
            return;
        };
        session.cancel.store(true, Ordering::SeqCst);
        self.clear_sheet_if(machine, session.tool);
        {
            let mut entries = self.entries();
            if matches!(entries.get(machine), Some(Run::Running(_))) {
                entries.insert(machine.to_owned(), Run::Running(stopping.to_owned()));
            }
        }
        self.changed();
    }

    pub fn close_session(&self, machine: &str, owner: &Arc<AtomicBool>) {
        let removed = {
            let mut sessions = self.sessions();
            match sessions.get(machine) {
                Some(session) if Arc::ptr_eq(&session.cancel, owner) => {
                    sessions.remove(machine).is_some()
                }
                _ => false,
            }
        };
        if removed {
            self.changed();
        }
    }

    pub fn set_sheet(&self, machine: &str, tool: Tool, node: &str) {
        *locked(&self.sheet) = Some(Sheet {
            machine: machine.to_owned(),
            tool,
            node: node.to_owned(),
        });
        self.changed();
    }

    pub fn sheet(&self) -> Option<Sheet> {
        locked(&self.sheet).clone()
    }

    pub fn clear_sheet(&self) {
        let had = locked(&self.sheet).take().is_some();
        if had {
            self.changed();
        }
    }

    pub fn clear_sheet_if(&self, machine: &str, tool: Tool) {
        let cleared = {
            let mut sheet = locked(&self.sheet);
            if sheet
                .as_ref()
                .is_some_and(|s| s.machine == machine && s.tool == tool)
            {
                *sheet = None;
                true
            } else {
                false
            }
        };
        if cleared {
            self.changed();
        }
    }

    pub fn mark_login(&self, machine: &str, tool: Tool, signed: bool, now_ms: i64) {
        locked(&self.logins).insert((machine.to_owned(), tool), (now_ms, signed));
        self.changed();
    }

    pub fn mark_login_run(&self, machine: &str, id: u64, tool: Tool, signed: bool, now_ms: i64) {
        if self.is_current(machine, id) {
            self.mark_login(machine, tool, signed, now_ms);
        }
    }

    pub fn login_mark(&self, machine: &str, tool: Tool) -> Option<(i64, bool)> {
        locked(&self.logins)
            .get(&(machine.to_owned(), tool))
            .copied()
    }

    pub fn mark_installed(&self, machine: &str, tool: Tool, version: Option<Version>, now_ms: i64) {
        locked(&self.installed).insert((machine.to_owned(), tool), (now_ms, version));
        self.changed();
    }

    pub fn installed_since(&self, machine: &str, tool: Tool) -> Option<Installed> {
        locked(&self.installed)
            .get(&(machine.to_owned(), tool))
            .cloned()
    }
}
