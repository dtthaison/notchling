//! Antigravity CLI sessions over `agy acp` (Swift `ACPRunner` / `AntigravityRunner`): one process per
//! opened session, which loads the conversation (`session/load` replays it), takes prompts,
//! streams the turn, asks permissions and switches options. A process nobody has looked at for ten
//! minutes, with no turn running and no permission waiting, is closed by [`Runner::reap`].
//!
//! Nothing is auto-approved, and `initialize` offers no file system or terminal, so any other
//! call from the agent gets `-32601`. What the runner learns goes out through a [`Sink`], on the
//! connection's thread: a sink must not call back into the runner's session it is told about while
//! holding a lock the runner needs.
//!
//! Locks are never held across a call into the connection (its callbacks may run inline).

use super::config::{AgyCommand, ConfigOption, Permission};
use super::live::{cancelled, Inner, Job};
use crate::tools::acp::Spawner;
use crate::transcript::model::Conversation;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The tool's name in messages.
pub const NAME: &str = "Antigravity";

pub const NO_BINARY: &str = "Couldn't find the agy command. Install Antigravity CLI first.";

/// Seconds since the epoch.
pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

/// When things are given up on, in seconds.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// A process unused this long is closed (10 minutes).
    pub idle: f64,
    /// A load that has not answered in this long fails (a hung process).
    pub load: f64,
    /// A failed load is shown this long before opening the session tries again.
    pub retry: f64,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            idle: 600.0,
            load: 60.0,
            retry: 15.0,
        }
    }
}

/// What happened to a session; `session` is `agy`'s own id, not Chat's row id.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// The conversation has loaded.
    Loaded,
    LoadFailed(String),
    /// Text or a tool call arrived. Frequent: a sink may thin these out.
    Chunk,
    TurnStarted,
    /// The turn ended; the error if it failed (the process died, the request was refused).
    TurnEnded(Option<String>),
    PermissionPending(Permission),
    /// A permission is no longer waiting: `outcome` is the chosen option's kind, or `cancelled`.
    PermissionResolved {
        rpc_id: String,
        outcome: String,
    },
    /// The options (model, thinking level, mode) changed.
    Options,
    OptionFailed(String),
    /// The process is gone; reopening the session starts a new one.
    Closed,
}

pub trait Sink: Send + Sync {
    fn changed(&self, session: &str, change: Change);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    NoBinary,
    /// A turn is running; one at a time.
    Busy,
    /// Not open, or not the permission that is waiting, or not a value the session offers.
    Refused(String),
    Start(String),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::NoBinary => f.write_str(NO_BINARY),
            RunError::Busy => f.write_str("a turn is running; send again when it ends"),
            RunError::Refused(why) | RunError::Start(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for RunError {}

/// The sessions open over `agy acp`. Cheap to clone.
#[derive(Clone)]
pub struct Runner {
    inner: Arc<Inner>,
}

impl Runner {
    /// `binary` is the `agy` to run (None: nothing can start, with [`NO_BINARY`]).
    pub fn new(
        spawner: Arc<dyn Spawner>,
        binary: Option<String>,
        sink: Arc<dyn Sink>,
        now: Clock,
        timing: Timing,
    ) -> Runner {
        Runner {
            inner: Arc::new(Inner {
                spawner,
                binary,
                sink,
                now,
                timing,
                live: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Starts a process for the session and loads it, unless one is open already. A load that
    /// failed is tried again once the retry time has passed, or at once with `retry_now`.
    pub fn open(&self, id: &str, folder: &str, retry_now: bool) -> Result<(), RunError> {
        self.inner.open(id, folder, retry_now).map(|_| ())
    }

    /// A new session in `folder`; waits up to `wait` for its id.
    pub fn start(&self, folder: &str, wait: Duration) -> Result<String, RunError> {
        let inner = &self.inner;
        let live = inner.launch(folder)?;
        let (tx, rx) = channel();
        let abandoned = Arc::new(AtomicBool::new(false));
        let (runner, l, gone) = (inner.clone(), live.clone(), abandoned.clone());
        let tx_init = tx.clone();
        let l_init = live.clone();
        inner.initialize(
            &live,
            Box::new(move |ok| {
                if !ok {
                    let why = l_init.lock().load_error.clone().unwrap_or_default();
                    l_init.conn.close();
                    let _ = tx_init.send(Err(RunError::Start(why)));
                    return;
                }
                let params = json!({"cwd": l.folder, "mcpServers": []});
                let (runner2, l2) = (runner.clone(), l.clone());
                l.conn.request(
                    "session/new",
                    params,
                    Box::new(move |result| {
                        let made = result.and_then(|r| {
                            let id = r.get("sessionId").and_then(Value::as_str).map(String::from);
                            id.map(|id| (id, r))
                                .ok_or_else(|| format!("{NAME} didn't start a session."))
                        });
                        match made {
                            Ok((id, r)) if !gone.load(Ordering::SeqCst) => {
                                l2.lock().session_id = Some(id.clone());
                                runner2.lock_live().insert(id.clone(), l2.clone());
                                runner2.loaded(&l2, &r);
                                let _ = tx.send(Ok(id));
                            }
                            Ok(_) => l2.conn.close(),
                            Err(e) => {
                                l2.conn.close();
                                let _ = tx.send(Err(RunError::Start(e)));
                            }
                        }
                    }),
                );
            }),
        );
        match rx.recv_timeout(wait) {
            Ok(result) => result,
            Err(_) => {
                abandoned.store(true, Ordering::SeqCst);
                live.conn.close();
                let mut map = inner.lock_live();
                map.retain(|_, l| !Arc::ptr_eq(l, &live));
                Err(RunError::Start(format!("{NAME} didn't start in time.")))
            }
        }
    }

    /// Sends a prompt and returns at once; the turn's end comes as [`Change::TurnEnded`]. One turn
    /// at a time per session: while one runs this is [`RunError::Busy`].
    pub fn prompt(&self, id: &str, text: &str, folder: &str) -> Result<(), RunError> {
        let inner = &self.inner;
        // Sending is a clear request to try again after a failed load.
        let retry = inner.get(id).is_some_and(|l| l.lock().failed_at.is_some());
        let live = inner.open(id, folder, retry)?;
        {
            let mut st = live.lock();
            if st.working {
                return Err(RunError::Busy);
            }
            st.working = true;
            st.last_used = (inner.now)();
        }
        let (runner, l, id, text) = (
            inner.clone(),
            live.clone(),
            id.to_string(),
            text.to_string(),
        );
        let job: Job = Box::new(move || runner.begin_prompt(&l, &id, &text));
        let now = {
            let mut st = live.lock();
            if st.loaded || st.load_error.is_some() {
                Some(job)
            } else {
                st.when_loaded.push(job);
                None
            }
        };
        if let Some(job) = now {
            job();
        }
        Ok(())
    }

    /// Stops the running turn; what was said so far stays. A waiting permission is answered as
    /// cancelled first.
    pub fn cancel(&self, id: &str) {
        let Some(live) = self.inner.get(id) else {
            return;
        };
        let pending = {
            let mut st = live.lock();
            if !st.working {
                return;
            }
            st.permission.take()
        };
        if let Some(p) = pending {
            live.conn.respond(&p.rpc_id, cancelled());
            self.inner.emit(
                &live,
                Change::PermissionResolved {
                    rpc_id: p.rpc_text(),
                    outcome: "cancelled".into(),
                },
            );
        }
        live.conn.notify("session/cancel", json!({"sessionId": id}));
    }

    /// Answers the waiting permission `rpc_id` with one of its options (None: cancelled). Returns
    /// the outcome, the option's kind or `cancelled`. Refused when that permission isn't waiting
    /// (answered already, or the turn ended) or the option isn't one of its own.
    pub fn answer(&self, id: &str, rpc_id: &str, option: Option<&str>) -> Result<String, RunError> {
        let live = self
            .inner
            .get(id)
            .ok_or_else(|| RunError::Refused(format!("{NAME} session isn't open")))?;
        let (permission, outcome) = {
            let mut st = live.lock();
            let waiting = st
                .permission
                .as_ref()
                .filter(|p| p.rpc_text() == rpc_id)
                .ok_or_else(|| RunError::Refused("no permission with that id is waiting".into()))?;
            let outcome = match option {
                None => "cancelled".to_string(),
                Some(o) => waiting
                    .option(o)
                    .map(|o| o.kind.clone())
                    .ok_or_else(|| RunError::Refused("that isn't one of its options".into()))?,
            };
            (st.permission.take(), outcome)
        };
        let Some(permission) = permission else {
            return Err(RunError::Refused("no permission is waiting".into()));
        };
        let result = match option {
            Some(o) => json!({"outcome": {"outcome": "selected", "optionId": o}}),
            None => cancelled(),
        };
        live.conn.respond(&permission.rpc_id, result);
        self.inner.emit(
            &live,
            Change::PermissionResolved {
                rpc_id: permission.rpc_text(),
                outcome: outcome.clone(),
            },
        );
        Ok(outcome)
    }

    /// Picks a value for one of the session's options; applies from the next turn. A failure comes
    /// as [`Change::OptionFailed`].
    pub fn set_option(&self, id: &str, option: &str, value: &str) -> Result<(), RunError> {
        let live = self
            .inner
            .get(id)
            .ok_or_else(|| RunError::Refused(format!("{NAME} session isn't open")))?;
        {
            let st = live.lock();
            let known =
                st.config.iter().find(|o| o.id == option).ok_or_else(|| {
                    RunError::Refused(format!("the session has no option {option}"))
                })?;
            if !known.values.iter().any(|v| v.value == value) {
                return Err(RunError::Refused(format!(
                    "{value} isn't a value of {option}"
                )));
            }
        }
        let (runner, l, option, value) = (
            self.inner.clone(),
            live.clone(),
            option.to_string(),
            value.to_string(),
        );
        let params = json!({"sessionId": id, "configId": option, "value": value});
        live.conn.request(
            "session/set_config_option",
            params,
            Box::new(move |result| match result {
                Ok(r) => {
                    {
                        let mut st = l.lock();
                        let fresh = ConfigOption::parse(r.get("configOptions"));
                        if !fresh.is_empty() {
                            st.config = fresh;
                        } else if let Some(o) = st.config.iter_mut().find(|o| o.id == option) {
                            o.current = value.clone();
                        }
                        if option == "model" {
                            let name = st
                                .config
                                .iter()
                                .find(|o| o.id == "model")
                                .map(|o| o.current_name().to_string());
                            st.transcript.model = name;
                        }
                    }
                    runner.emit(&l, Change::Options);
                }
                Err(e) => runner.emit(&l, Change::OptionFailed(e)),
            }),
        );
        Ok(())
    }

    /// Ends the process for a session (it is reloaded when opened again).
    pub fn close(&self, id: &str) {
        self.inner.close(id);
    }

    /// No `agy` process left behind.
    pub fn close_all(&self) {
        let ids: Vec<String> = self.inner.lock_live().keys().cloned().collect();
        for id in ids {
            self.inner.close(&id);
        }
    }

    /// Closes what has sat unused past the idle time (no turn, no waiting permission) and fails a
    /// load that has not answered in the load time. Called every so often.
    pub fn reap(&self) {
        let inner = &self.inner;
        let now = (inner.now)();
        let mut hung = Vec::new();
        let mut idle = Vec::new();
        for (id, l) in inner.lock_live().iter() {
            let st = l.lock();
            if !st.loaded && st.load_error.is_none() && now - st.opened_at > inner.timing.load {
                hung.push(l.clone());
            } else if !st.working
                && st.permission.is_none()
                && now - st.last_used > inner.timing.idle
            {
                idle.push(id.clone());
            }
        }
        for l in hung {
            inner.fail_load(&l, &format!("{NAME} didn't answer in time."));
        }
        for id in idle {
            inner.close(&id);
        }
    }

    // MARK: Reads

    pub fn is_open(&self, id: &str) -> bool {
        self.inner.lock_live().contains_key(id)
    }

    pub fn is_working(&self, id: &str) -> bool {
        self.inner.get(id).is_some_and(|l| l.lock().working)
    }

    /// The open sessions with a turn running.
    pub fn working(&self) -> Vec<String> {
        self.inner
            .lock_live()
            .iter()
            .filter(|(_, l)| l.lock().working)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn is_loading(&self, id: &str) -> bool {
        self.inner.get(id).is_some_and(|l| {
            let st = l.lock();
            !st.loaded && st.load_error.is_none()
        })
    }

    pub fn load_error(&self, id: &str) -> Option<String> {
        self.inner.get(id).and_then(|l| l.lock().load_error.clone())
    }

    pub fn permission(&self, id: &str) -> Option<Permission> {
        self.inner.get(id).and_then(|l| l.lock().permission.clone())
    }

    pub fn config(&self, id: &str) -> Vec<ConfigOption> {
        self.inner
            .get(id)
            .map(|l| l.lock().config.clone())
            .unwrap_or_default()
    }

    pub fn commands(&self, id: &str) -> Vec<AgyCommand> {
        self.inner
            .get(id)
            .map(|l| l.lock().commands.clone())
            .unwrap_or_default()
    }

    /// Antigravity's own title for the conversation, once it has named it.
    pub fn title(&self, id: &str) -> Option<String> {
        self.inner
            .get(id)
            .and_then(|l| l.lock().transcript.title().map(String::from))
    }

    /// The folder the session runs in.
    pub fn folder(&self, id: &str) -> Option<String> {
        self.inner.get(id).map(|l| l.folder.clone())
    }

    /// The conversation as far as it has loaded, counting as use.
    pub fn conversation(&self, id: &str) -> Option<Conversation> {
        let live = self.inner.get(id)?;
        let mut st = live.lock();
        st.last_used = (self.inner.now)();
        Some(st.transcript.conversation())
    }

    /// The same without counting as use: a list read every few seconds must not keep an idle
    /// process from being reaped.
    pub fn peek_conversation(&self, id: &str) -> Option<Conversation> {
        let live = self.inner.get(id)?;
        let conversation = live.lock().transcript.conversation();
        Some(conversation)
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
