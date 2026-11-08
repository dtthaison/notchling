//! The sessions the host owns (`SessionHost.swift`): starts them, remembers them, finds the Claude
//! session behind each, brings dead ones back after a restart, and hands out the pty for attaching.
//! No sockets here; `server` puts this on one.

use crate::state::{HostRecord, StateStore};
use crate::wire::SessionId;
use crate::HostError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

/// Gets output: the scrollback first (one chunk), then live bytes.
pub type Listener = Arc<dyn Fn(&[u8]) + Send + Sync>;
/// Runs once when the session's process ends.
pub type ExitHandler = Box<dyn FnOnce() + Send>;

/// A pty the host can show and type into (`TerminalHandle`): the real one is
/// [`crate::pty_handle::PtyHandle`]; tests pass a fake.
pub trait Handle: Send + Sync {
    fn id(&self) -> SessionId;
    fn cwd(&self) -> &Path;
    fn model(&self) -> Option<&str>;
    fn started_at_ms(&self) -> i64;
    fn root_pid(&self) -> i32;
    fn is_running(&self) -> bool;
    /// Runs `handler` when the process ends, or now if it already has.
    fn on_exit(&self, handler: ExitHandler);
    /// The listener gets everything already on screen first (one chunk), then live output.
    fn add_listener(&self, listener: Listener) -> u64;
    fn remove_listener(&self, token: u64);
    /// Queued: a pty that isn't reading never blocks the caller.
    fn write(&self, bytes: &[u8]);
    fn resize(&self, columns: u16, rows: u16);
    /// Asks the process to end (SIGTERM, then SIGKILL after a moment).
    fn terminate(&self);
}

/// How the host starts a process on a pty.
pub trait Spawner: Send + Sync {
    fn spawn(
        &self,
        executable: &Path,
        arguments: &[String],
        cwd: &Path,
        model: Option<&str>,
    ) -> Result<Arc<dyn Handle>, HostError>;
    /// The `claude` executable.
    fn claude_executable(&self) -> Result<PathBuf, HostError>;
}

/// A session as `list` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedInfo {
    pub id: SessionId,
    pub cwd: String,
    pub model: Option<String>,
    pub pid: i32,
    pub started_at_ms: i64,
    pub running: bool,
    pub claude_session_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostEvent {
    Exited { id: SessionId, status: i64 },
    Spawned { id: SessionId },
}

/// What the host asks the system, and how hard it looks for a new session's Claude id.
#[derive(Clone)]
pub struct HostOptions {
    pub is_alive: Arc<dyn Fn(i32) -> bool + Send + Sync>,
    /// The Claude session a pty's process became, from its root pid.
    pub discover_session_id: Arc<dyn Fn(i32) -> Option<String> + Send + Sync>,
    pub discovery_interval: Duration,
    pub discovery_attempts: usize,
}

impl Default for HostOptions {
    /// The live process check, no discovery, and the Swift host's 1 s × 600 schedule.
    fn default() -> Self {
        HostOptions {
            is_alive: Arc::new(crate::system::is_process_alive),
            discover_session_id: Arc::new(|_| None),
            discovery_interval: Duration::from_secs(1),
            discovery_attempts: 600,
        }
    }
}

type EventSink = Arc<dyn Fn(HostEvent) + Send + Sync>;

#[derive(Default)]
struct Sessions {
    handles: HashMap<SessionId, Arc<dyn Handle>>,
    claude_ids: HashMap<SessionId, String>,
}

pub struct SessionHost {
    spawner: Arc<dyn Spawner>,
    pub state: Arc<StateStore>,
    options: HostOptions,
    sessions: Mutex<Sessions>,
    on_event: Mutex<Option<EventSink>>,
    this: Weak<SessionHost>,
}

impl SessionHost {
    pub fn new(
        spawner: Arc<dyn Spawner>,
        state: Arc<StateStore>,
        options: HostOptions,
    ) -> Arc<SessionHost> {
        Arc::new_cyclic(|this| SessionHost {
            spawner,
            state,
            options,
            sessions: Mutex::new(Sessions::default()),
            on_event: Mutex::new(None),
            this: this.clone(),
        })
    }

    /// Where `spawned` and `exit` events go (the server broadcasts them).
    pub fn set_on_event(&self, sink: impl Fn(HostEvent) + Send + Sync + 'static) {
        *self.on_event.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(sink));
    }

    fn emit(&self, event: HostEvent) {
        let sink = self
            .on_event
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(sink) = sink {
            sink(event);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Sessions> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner())
    }

    // Starting

    pub fn spawn_claude(&self, cwd: &Path, model: Option<&str>) -> Result<HostedInfo, HostError> {
        let arguments: Vec<String> = model
            .map(|m| vec!["--model".to_string(), m.to_string()])
            .unwrap_or_default();
        let claude = self.spawner.claude_executable()?;
        self.start(&claude, &arguments, cwd, model, None, true)
    }

    pub fn resume(&self, session_id: &str, cwd: &Path) -> Result<HostedInfo, HostError> {
        let arguments = vec!["--resume".to_string(), session_id.to_string()];
        let claude = self.spawner.claude_executable()?;
        self.start(&claude, &arguments, cwd, None, Some(session_id), true)
    }

    /// Any command on a pty (tests, the demo terminal). Not recorded: it cannot be resumed.
    pub fn spawn_command(
        &self,
        executable: &Path,
        arguments: &[String],
        cwd: &Path,
    ) -> Result<HostedInfo, HostError> {
        self.start(executable, arguments, cwd, None, None, false)
    }

    fn start(
        &self,
        executable: &Path,
        arguments: &[String],
        cwd: &Path,
        model: Option<&str>,
        claude_session_id: Option<&str>,
        record: bool,
    ) -> Result<HostedInfo, HostError> {
        let handle = self.spawner.spawn(executable, arguments, cwd, model)?;
        let id = handle.id();
        {
            let mut sessions = self.lock();
            sessions.handles.insert(id, handle.clone());
            if let Some(claude) = claude_session_id {
                sessions.claude_ids.insert(id, claude.to_string());
            }
        }
        if record {
            let entry = HostRecord {
                id: id.to_string(),
                cwd: cwd.to_string_lossy().into_owned(),
                model: model.map(String::from),
                claude_session_id: claude_session_id.map(String::from),
                pid: handle.root_pid(),
                started_at_ms: handle.started_at_ms(),
            };
            self.state.upsert(entry.clone());
            if claude_session_id.is_none() {
                self.watch_for_claude_session(handle.clone(), entry);
            }
        }
        let this = self.this.clone();
        handle.on_exit(Box::new(move || {
            if let Some(host) = this.upgrade() {
                host.exited(id);
            }
        }));
        self.emit(HostEvent::Spawned { id });
        Ok(self.info(handle.as_ref()))
    }

    /// The session file appears once Claude is past its startup prompts; remember its id so the
    /// session can be resumed.
    fn watch_for_claude_session(&self, handle: Arc<dyn Handle>, record: HostRecord) {
        let this = self.this.clone();
        let discover = self.options.discover_session_id.clone();
        let interval = self.options.discovery_interval;
        let attempts = self.options.discovery_attempts;
        std::thread::spawn(move || {
            for _ in 0..attempts {
                if !handle.is_running() {
                    return;
                }
                if let Some(claude) = discover(handle.root_pid()) {
                    let Some(host) = this.upgrade() else { return };
                    host.lock().claude_ids.insert(handle.id(), claude.clone());
                    host.state.upsert(HostRecord {
                        claude_session_id: Some(claude),
                        ..record
                    });
                    return;
                }
                std::thread::sleep(interval);
            }
        });
    }

    fn exited(&self, id: SessionId) {
        // The process ended by itself (or was ended on purpose): there is nothing to bring back.
        self.state.remove(&id.to_string());
        {
            let mut sessions = self.lock();
            sessions.handles.remove(&id);
            sessions.claude_ids.remove(&id);
        }
        self.emit(HostEvent::Exited { id, status: 0 });
    }

    // Asking

    fn info(&self, handle: &dyn Handle) -> HostedInfo {
        let claude = self.lock().claude_ids.get(&handle.id()).cloned();
        HostedInfo {
            id: handle.id(),
            cwd: handle.cwd().to_string_lossy().into_owned(),
            model: handle.model().map(String::from),
            pid: handle.root_pid(),
            started_at_ms: handle.started_at_ms(),
            running: handle.is_running(),
            claude_session_id: claude,
        }
    }

    pub fn list(&self) -> Vec<HostedInfo> {
        let all: Vec<Arc<dyn Handle>> = self.lock().handles.values().cloned().collect();
        let mut infos: Vec<HostedInfo> = all.iter().map(|h| self.info(h.as_ref())).collect();
        infos.sort_by_key(|i| i.started_at_ms);
        infos
    }

    pub fn running_count(&self) -> usize {
        self.list().iter().filter(|i| i.running).count()
    }

    pub fn session(&self, id: SessionId) -> Option<Arc<dyn Handle>> {
        self.lock().handles.get(&id).cloned()
    }

    pub fn terminate(&self, id: SessionId) {
        let Some(handle) = self.session(id) else {
            return;
        };
        self.state.remove(&id.to_string());
        handle.terminate();
    }

    // After a restart

    /// Brings back recorded sessions whose process is gone: `claude --resume <id>` in the same
    /// folder. Records without a known Claude session id, or whose folder is gone, are dropped.
    /// Returns how many were resumed.
    pub fn resume_recorded(&self) -> usize {
        if !self.state.settings().resume_on_start {
            return 0;
        }
        let mut resumed = 0;
        for record in self.state.records() {
            let alive = record.pid > 1 && (self.options.is_alive)(record.pid);
            if alive {
                continue;
            }
            self.state.remove(&record.id);
            let Some(claude) = record.claude_session_id else {
                eprintln!(
                    "hostd: {} has no Claude session id, not resuming",
                    record.id
                );
                continue;
            };
            if !Path::new(&record.cwd).is_dir() {
                eprintln!("hostd: folder of {claude} is gone, not resuming");
                continue;
            }
            match self.resume(&claude, Path::new(&record.cwd)) {
                Ok(_) => resumed += 1,
                Err(error) => eprintln!("hostd: resuming {claude}: {error}"),
            }
        }
        resumed
    }

    /// Writes a record for a session that is running outside the host right now, to be resumed when
    /// the host starts (the migration of sessions that live inside an old Agent Console).
    pub fn record_for_resume(&self, claude_session_id: &str, cwd: &str, model: Option<&str>) {
        record_for_resume(&self.state, claude_session_id, cwd, model);
    }
}

/// `--record-resume`: the same record, without a host.
pub fn record_for_resume(
    state: &StateStore,
    claude_session_id: &str,
    cwd: &str,
    model: Option<&str>,
) {
    state.upsert(HostRecord {
        id: SessionId::random().to_string(),
        cwd: cwd.to_string(),
        model: model.map(String::from),
        claude_session_id: Some(claude_session_id.to_string()),
        pid: 0,
        started_at_ms: crate::system::now_ms(),
    });
}
