//! The open sessions behind [`super::Runner`]: one [`Live`] per session (its connection and state),
//! the shared [`Inner`] that opens, loads and closes them, and [`Wire`], what a connection hears.

use super::config::{rpc_text, AgyCommand, ConfigOption, Permission};
use super::runner::{Change, Clock, RunError, Sink, Timing, NAME};
use crate::tools::acp::{Connection, Handler, Spawner, METHOD_NOT_FOUND};
use crate::transcript::acp::Transcript;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

/// How many `agy` processes run at once, over all sessions.
pub const MAX_OPEN: usize = 8;

pub(super) type Job = Box<dyn FnOnce() + Send>;

pub(super) struct Live {
    pub(super) conn: Connection,
    pub(super) folder: String,
    pub(super) st: Mutex<St>,
}

#[derive(Default)]
pub(super) struct St {
    pub(super) session_id: Option<String>,
    pub(super) transcript: Transcript,
    pub(super) loaded: bool,
    pub(super) working: bool,
    pub(super) permission: Option<Permission>,
    pub(super) config: Vec<ConfigOption>,
    pub(super) commands: Vec<AgyCommand>,
    pub(super) opened_at: f64,
    pub(super) last_used: f64,
    pub(super) failed_at: Option<f64>,
    pub(super) load_error: Option<String>,
    /// Work waiting for the load to finish.
    pub(super) when_loaded: Vec<Job>,
}

impl Live {
    pub(super) fn lock(&self) -> MutexGuard<'_, St> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub(super) struct Inner {
    pub(super) spawner: Arc<dyn Spawner>,
    pub(super) binary: Option<String>,
    pub(super) sink: Arc<dyn Sink>,
    pub(super) now: Clock,
    pub(super) timing: Timing,
    pub(super) live: Mutex<HashMap<String, Arc<Live>>>,
    /// How many processes may run at once ([`MAX_OPEN`] unless a test sets less).
    pub(super) max_open: AtomicUsize,
    /// Asked before each process is started: the daemon's `acting` switch (without it any may).
    pub(super) may_start: OnceLock<Arc<dyn Fn() -> bool + Send + Sync>>,
}

pub(super) fn cancelled() -> Value {
    json!({"outcome": {"outcome": "cancelled"}})
}

impl Inner {
    pub(super) fn lock_live(&self) -> MutexGuard<'_, HashMap<String, Arc<Live>>> {
        self.live.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(super) fn get(&self, id: &str) -> Option<Arc<Live>> {
        self.lock_live().get(id).cloned()
    }

    pub(super) fn emit(&self, live: &Live, change: Change) {
        let id = live.lock().session_id.clone();
        if let Some(id) = id {
            self.sink.changed(&id, change);
        }
    }

    pub(super) fn close(&self, id: &str) {
        let removed = self.lock_live().remove(id);
        if let Some(live) = removed {
            self.cancel_waiting(&live, id);
            live.conn.close();
            self.sink.changed(id, Change::Closed);
        }
    }

    /// A session that goes away with a permission waiting in it: nothing can answer that any more,
    /// so it ends as cancelled, before the session's `Closed`.
    pub(super) fn cancel_waiting(&self, live: &Live, id: &str) {
        let waiting = live.lock().permission.take();
        if let Some(p) = waiting {
            self.sink.changed(
                id,
                Change::PermissionResolved {
                    rpc_id: p.rpc_text(),
                    outcome: "cancelled".into(),
                },
            );
        }
    }

    /// Whether another process may start: fewer than `max_open` are running.
    fn make_room(&self, map: &HashMap<String, Arc<Live>>) -> Result<(), RunError> {
        let max = self.max_open.load(Ordering::SeqCst);
        if map.values().filter(|l| !l.conn.is_closed()).count() >= max {
            return Err(RunError::Refused(format!(
                "{max} {NAME} sessions are open already; close one first"
            )));
        }
        Ok(())
    }

    pub(super) fn open(
        self: &Arc<Self>,
        id: &str,
        folder: &str,
        retry_now: bool,
    ) -> Result<Arc<Live>, RunError> {
        let now = (self.now)();
        let stale = {
            let mut map = self.lock_live();
            if let Some(l) = map.get(id).cloned() {
                let failed_at = {
                    let mut st = l.lock();
                    if st.failed_at.is_none() {
                        st.last_used = now;
                    }
                    st.failed_at
                };
                match failed_at {
                    Some(at) if retry_now || now - at > self.timing.retry => {
                        map.remove(id);
                        Some(l)
                    }
                    _ => {
                        l.lock().last_used = now;
                        return Ok(l);
                    }
                }
            } else {
                None
            }
        };
        if let Some(old) = stale {
            old.conn.close();
        }
        let live = self.launch(folder)?;
        live.lock().session_id = Some(id.to_string());
        {
            let mut map = self.lock_live();
            // Two opens at once: the first wins, the second's process is not kept.
            if let Some(existing) = map.get(id).cloned() {
                drop(map);
                live.conn.close();
                return Ok(existing);
            }
            // Or the last room went to someone else while this one started.
            if let Err(full) = self.make_room(&map) {
                drop(map);
                live.conn.close();
                return Err(full);
            }
            map.insert(id.to_string(), live.clone());
        }
        let (runner, l, session, cwd) = (
            self.clone(),
            live.clone(),
            id.to_string(),
            folder.to_string(),
        );
        self.initialize(
            &live,
            Box::new(move |ok| {
                if !ok {
                    return;
                }
                let params = json!({"sessionId": session, "cwd": cwd, "mcpServers": []});
                let (runner2, l2) = (runner.clone(), l.clone());
                l.conn.request(
                    "session/load",
                    params,
                    Box::new(move |result| match result {
                        Ok(r) => runner2.loaded(&l2, &r),
                        Err(e) => runner2.fail_load(&l2, &e),
                    }),
                );
            }),
        );
        Ok(live)
    }

    /// Starts the process and its reader; nothing is sent yet.
    pub(super) fn launch(self: &Arc<Self>, folder: &str) -> Result<Arc<Live>, RunError> {
        let binary = self.binary.clone().ok_or(RunError::NoBinary)?;
        if self.may_start.get().is_some_and(|may| !may()) {
            return Err(RunError::NotAllowed);
        }
        self.make_room(&self.lock_live())?;
        let spawned = self
            .spawner
            .spawn(&binary, &["acp".to_string()], folder)
            .map_err(|e| RunError::Start(format!("Couldn't start {NAME}: {e}")))?;
        let conn = Connection::new(spawned, NAME);
        let now = (self.now)();
        let live = Arc::new(Live {
            conn: conn.clone(),
            folder: folder.to_string(),
            st: Mutex::new(St {
                opened_at: now,
                last_used: now,
                ..St::default()
            }),
        });
        conn.start(Arc::new(Wire {
            live: Arc::downgrade(&live),
            runner: Arc::downgrade(self),
        }));
        Ok(live)
    }

    /// `initialize`, offering no file system and no terminal; `then` runs with whether it worked.
    pub(super) fn initialize(
        self: &Arc<Self>,
        live: &Arc<Live>,
        then: Box<dyn FnOnce(bool) + Send>,
    ) {
        let params = json!({
            "protocolVersion": 1,
            "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
            "clientInfo": {"name": "Notchling", "version": "1"},
        });
        let (runner, l) = (self.clone(), live.clone());
        live.conn.request(
            "initialize",
            params,
            Box::new(move |result| match result {
                Ok(_) => then(true),
                Err(e) => {
                    let known = l.lock().session_id.is_some();
                    if known {
                        runner.fail_load(&l, &e);
                    } else {
                        l.lock().load_error = Some(e);
                    }
                    then(false);
                }
            }),
        );
    }

    pub(super) fn loaded(&self, live: &Arc<Live>, result: &Value) {
        let waiting = {
            let mut st = live.lock();
            st.loaded = true;
            st.config = ConfigOption::parse(result.get("configOptions"));
            let model = st
                .config
                .iter()
                .find(|o| o.id == "model")
                .map(|o| o.current_name().to_string())
                .or_else(|| {
                    result
                        .pointer("/models/currentModelId")
                        .and_then(Value::as_str)
                        .map(String::from)
                });
            if model.is_some() {
                st.transcript.model = model;
            }
            std::mem::take(&mut st.when_loaded)
        };
        for job in waiting {
            job();
        }
        self.emit(live, Change::Loaded);
    }

    /// Loading failed: the error stays on show, sends that waited for the load end with it (they
    /// would otherwise wait forever), and the process is closed so a later open starts a new one.
    pub(super) fn fail_load(&self, live: &Arc<Live>, message: &str) {
        let waiting = {
            let mut st = live.lock();
            if st.loaded || st.failed_at.is_some() {
                return;
            }
            st.load_error = Some(message.to_string());
            st.failed_at = Some((self.now)());
            std::mem::take(&mut st.when_loaded)
        };
        for job in waiting {
            job();
        }
        live.conn.close();
        self.emit(live, Change::LoadFailed(message.to_string()));
    }

    pub(super) fn begin_prompt(self: &Arc<Self>, live: &Arc<Live>, id: &str, text: &str) {
        let now = (self.now)();
        let failed = {
            let mut st = live.lock();
            match st.load_error.clone() {
                Some(e) => {
                    st.working = false;
                    Some(e)
                }
                None => {
                    st.last_used = now;
                    st.transcript.begin_turn(now);
                    None
                }
            }
        };
        if let Some(e) = failed {
            self.emit(live, Change::TurnEnded(Some(e)));
            return;
        }
        self.emit(live, Change::TurnStarted);
        let (runner, l) = (self.clone(), live.clone());
        live.conn.request(
            "session/prompt",
            json!({"sessionId": id, "prompt": [{"type": "text", "text": text}]}),
            Box::new(move |result| runner.turn_done(&l, result)),
        );
    }

    pub(super) fn turn_done(&self, live: &Arc<Live>, result: Result<Value, String>) {
        let now = (self.now)();
        let (error, permission) = {
            let mut st = live.lock();
            st.working = false;
            st.last_used = now;
            let permission = st.permission.take();
            let error = match &result {
                Ok(r) => {
                    let reason = r.get("stopReason").and_then(Value::as_str);
                    st.transcript.end_turn(reason, r.get("usage"), now);
                    None
                }
                Err(e) => {
                    st.transcript.fail_turn(e, now);
                    Some(e.clone())
                }
            };
            (error, permission)
        };
        if let Some(p) = permission {
            self.emit(
                live,
                Change::PermissionResolved {
                    rpc_id: p.rpc_text(),
                    outcome: "cancelled".into(),
                },
            );
        }
        self.emit(live, Change::TurnEnded(error));
    }
}

/// What a connection hears, for one session.
pub(super) struct Wire {
    live: Weak<Live>,
    runner: Weak<Inner>,
}

impl Wire {
    pub(super) fn both(&self) -> Option<(Arc<Live>, Arc<Inner>)> {
        Some((self.live.upgrade()?, self.runner.upgrade()?))
    }
}

impl Handler for Wire {
    fn notification(&self, method: &str, params: &Value) {
        let Some((live, runner)) = self.both() else {
            return;
        };
        let Some(update) = params.get("update").filter(|u| u.is_object()) else {
            return;
        };
        if method != "session/update" {
            return;
        }
        {
            let mut st = live.lock();
            match update.get("sessionUpdate").and_then(Value::as_str) {
                Some("available_commands_update") => {
                    st.commands = AgyCommand::parse(update.get("availableCommands"));
                }
                Some("config_option_update") => {
                    let fresh = ConfigOption::parse(update.get("configOptions"));
                    if !fresh.is_empty() {
                        st.config = fresh;
                    }
                }
                Some("current_mode_update") => {
                    let mode = update.get("currentModeId").and_then(Value::as_str);
                    if let (Some(mode), Some(o)) =
                        (mode, st.config.iter_mut().find(|o| o.id == "mode"))
                    {
                        o.current = mode.to_string();
                    }
                }
                _ => {
                    let at = st.loaded.then(|| (runner.now)());
                    st.transcript.apply(update, at);
                }
            }
        }
        runner.emit(&live, Change::Chunk);
    }

    fn request(&self, id: &Value, method: &str, params: &Value) {
        let Some((live, runner)) = self.both() else {
            return;
        };
        let permission = if method == "session/request_permission" {
            Permission::parse(id, params)
        } else {
            None
        };
        let Some(permission) = permission else {
            // No file system or terminal access is offered in `initialize`; anything else is refused.
            live.conn
                .respond_error(id, METHOD_NOT_FOUND, "Not supported");
            return;
        };
        let replaced = {
            let mut st = live.lock();
            if let Some(call) = params.get("toolCall") {
                let mut update = call.clone();
                update["sessionUpdate"] = json!("tool_call_update");
                let at = (runner.now)();
                st.transcript.apply(&update, Some(at));
            }
            st.permission.replace(permission.clone())
        };
        // A second request while one waits: the first can't be answered any more, so it is
        // cancelled rather than left hanging in the agent.
        if let Some(old) = replaced {
            live.conn.respond(&old.rpc_id, cancelled());
            runner.emit(
                &live,
                Change::PermissionResolved {
                    rpc_id: rpc_text(&old.rpc_id),
                    outcome: "cancelled".into(),
                },
            );
        }
        runner.emit(&live, Change::PermissionPending(permission));
    }

    fn exit(&self, message: Option<String>) {
        let Some((live, runner)) = self.both() else {
            return;
        };
        let loaded = live.lock().loaded;
        if !loaded {
            runner.fail_load(
                &live,
                &message.unwrap_or_else(|| format!("{NAME} stopped.")),
            );
            return;
        }
        let id = live.lock().session_id.clone();
        if let Some(id) = id {
            let mut map = runner.lock_live();
            if map.get(&id).is_some_and(|l| Arc::ptr_eq(l, &live)) {
                map.remove(&id);
                drop(map);
                runner.cancel_waiting(&live, &id);
                runner.sink.changed(&id, Change::Closed);
            }
        }
    }
}
