//! Milestone 5's Antigravity CLI ops (E6b): `agy acp` sessions run by the daemon over
//! [`notchling_engine::tools::agy`]. Built and switched off: while [`Acting`] is off every op that
//! starts `agy`, types into it or answers it answers `forbidden` with [`ACTING_OFF`]; the reads
//! work.
//!
//! | Op | Tier | Acting | Args | Reply |
//! | --- | --- | --- | --- | --- |
//! | `agy.sessions` | agent | no | `{}` | `[{id: "agy:<id>", name, project, cwd, sinceMs, working}]` (the summaries copy, plus sessions started here the copy doesn't list yet) |
//! | `agy.state` | app | no | `{id}` | `{id, open, loading, loadError, working, title, folder, config, commands, permission}` |
//! | `agy.open` | app | yes | `{id}` | `{id, loading: true}`: starts `agy acp` in the session's own folder and loads it |
//! | `agy.send` | app | yes | `{id, text}` or `{draft_folder, text}` | `{id, sent: true}`; a draft starts the session first |
//! | `agy.cancel` | app | yes | `{id}` | `{cancelled: true}` |
//! | `agy.set_option` | app | yes | `{id, option, value}` | `{requested: true}`: the session's own options only |
//! | `agy.permission` | app | yes | `{id, rpc_id, option_id?}` | `{outcome}`; no `option_id` cancels |
//!
//! `id` is a row id, `agy:<session>`. A session's folder comes from the summaries copy or from the
//! session this daemon started, never from the caller; a draft's `draft_folder` is an absolute
//! folder that exists. One turn runs per session: `agy.send` while one runs is `failed` ("busy").
//!
//! Events: `session.changed {id, change, error?}` on `sessions` (chunks at most every 250 ms),
//! and on `prompts` (app tier) `agy.permission.pending` / `agy.permission.resolved`, each also as
//! `prompt.pending` / `prompt.resolved`, so the notch shares one answered state with Chat. A
//! pending permission is a `prompts.list` row too. Its id is `agy-perm:<row id>:<rpc id>`.
//! Nothing is answered by the daemon on its own; turning acting off closes every session, which
//! cancels what was waiting.

use crate::acting::{Acting, ACTING_OFF};
use crate::acting_ops::{now_ms, text_arg, Room};
use notchling_engine::bridge::foreign::PROMPT_PREFIX;
use notchling_engine::tools::acp::Spawner;
use notchling_engine::tools::agy::summaries::{self, Row};
use notchling_engine::tools::agy::{self, Change, Permission, RunError, Runner, Sink, Timing};
use notchling_local_api::{topics, ApiError, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const SESSIONS: &str = "agy.sessions";
pub const STATE: &str = "agy.state";
pub const OPEN: &str = "agy.open";
pub const SEND: &str = "agy.send";
pub const CANCEL: &str = "agy.cancel";
pub const SET_OPTION: &str = "agy.set_option";
pub const PERMISSION: &str = "agy.permission";

/// The event for a session's change (`sessions` topic).
pub const SESSION_CHANGED: &str = "session.changed";
pub const PERMISSION_PENDING: &str = "agy.permission.pending";
pub const PERMISSION_RESOLVED: &str = "agy.permission.resolved";

/// The ops that start, type into or answer `agy`: refused while acting is off.
pub const ACTING_OPS: [&str; 5] = [OPEN, SEND, CANCEL, SET_OPTION, PERMISSION];

/// Chunk events are thinned to one per session in this long.
const CHUNK_EVERY: Duration = Duration::from_millis(250);

/// Where `agy` lives and where its list is copied.
#[derive(Clone, Debug)]
pub struct AgySetup {
    /// `~/.gemini/antigravity-cli`: read only through a copy.
    pub root: PathBuf,
    /// Our own folder for that copy.
    pub copy_dir: PathBuf,
    /// The `agy` to run; None when it isn't installed.
    pub binary: Option<String>,
    /// How long `agy.send` waits for a new session's id (under the client's call timeout).
    pub start_wait: Duration,
    pub timing: Timing,
}

/// A session this daemon started that the summaries copy may not list yet.
struct Started {
    name: String,
    folder: String,
    at: f64,
}

/// What the runner reports, turned into events; also the permissions waiting.
struct Shared {
    room: Arc<dyn Room>,
    pending: Mutex<BTreeMap<String, Value>>,
    last_chunk: Mutex<HashMap<String, Instant>>,
    started: Mutex<HashMap<String, Started>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn row_id(session: &str) -> String {
    format!("agy:{session}")
}

fn prompt_id(session: &str, rpc: &str) -> String {
    format!("{PROMPT_PREFIX}{}:{rpc}", row_id(session))
}

impl Shared {
    fn publish_both(&self, event: &str, prompt_event: &str, data: &Value) {
        self.room.publish(topics::PROMPTS, event, data);
        self.room.publish(topics::PROMPTS, prompt_event, data);
    }

    fn session_changed(&self, session: &str, change: &str, error: Option<String>) {
        let mut data = json!({"id": row_id(session), "change": change});
        if let Some(error) = error {
            data["error"] = json!(error);
        }
        self.room.publish(topics::SESSIONS, SESSION_CHANGED, &data);
    }

    /// `prompt.pending`'s shape (as a hook prompt's) plus `tool`, `rpcId`, `options` and `diff`.
    fn prompt_json(&self, session: &str, p: &Permission, folder: &str) -> Value {
        let card = p.to_json();
        let summary = if p.call.title.is_empty() {
            p.call.name.clone()
        } else {
            p.call.title.clone()
        };
        let mut data = json!({
            "id": prompt_id(session, &p.rpc_text()),
            "sessionId": row_id(session),
            "tool": "agy",
            "kind": "permission",
            "toolName": p.call.name,
            "summary": summary,
            "input": p.call.input,
            "canAlwaysAllow": p.options.iter().any(|o| o.kind == "allow_always"),
            "question": null,
            "cwd": folder,
            "transcriptPath": "",
            "createdMs": now_ms(),
        });
        for key in ["rpcId", "options", "diff"] {
            data[key] = card[key].clone();
        }
        data
    }
}

/// A runner's sink that knows each session's folder through the runner.
struct DaemonSink {
    shared: Arc<Shared>,
    runner: std::sync::OnceLock<Runner>,
}

impl Sink for DaemonSink {
    fn changed(&self, session: &str, change: Change) {
        let shared = &self.shared;
        match change {
            Change::Chunk => {
                let mut last = lock(&shared.last_chunk);
                let due = last
                    .get(session)
                    .is_none_or(|at| at.elapsed() >= CHUNK_EVERY);
                if due {
                    last.insert(session.to_string(), Instant::now());
                    drop(last);
                    shared.session_changed(session, "chunk", None);
                }
            }
            Change::Loaded => shared.session_changed(session, "loaded", None),
            Change::LoadFailed(e) => shared.session_changed(session, "load_failed", Some(e)),
            Change::TurnStarted => shared.session_changed(session, "turn_started", None),
            Change::TurnEnded(error) => shared.session_changed(session, "turn_ended", error),
            Change::Options => shared.session_changed(session, "options", None),
            Change::OptionFailed(e) => shared.session_changed(session, "option_failed", Some(e)),
            Change::Closed => {
                lock(&shared.last_chunk).remove(session);
                shared.session_changed(session, "closed", None);
            }
            Change::PermissionPending(p) => {
                let folder = self
                    .runner
                    .get()
                    .and_then(|r| r.folder(session))
                    .unwrap_or_default();
                let data = shared.prompt_json(session, &p, &folder);
                let id = data["id"].as_str().unwrap_or_default().to_string();
                lock(&shared.pending).insert(id, data.clone());
                shared.publish_both(PERMISSION_PENDING, topics::PROMPT_PENDING, &data);
                shared.session_changed(session, "permission", None);
            }
            Change::PermissionResolved { rpc_id, outcome } => {
                let id = prompt_id(session, &rpc_id);
                lock(&shared.pending).remove(&id);
                let by = if outcome == "cancelled" {
                    "stopped"
                } else {
                    "app"
                };
                let data = json!({"id": id, "sessionId": row_id(session), "tool": "agy",
                                  "outcome": outcome, "by": by});
                shared.publish_both(PERMISSION_RESOLVED, topics::PROMPT_RESOLVED, &data);
                shared.session_changed(session, "permission_resolved", None);
            }
        }
    }
}

pub struct AgyOps {
    acting: Arc<Acting>,
    shared: Arc<Shared>,
    runner: Runner,
    setup: AgySetup,
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

fn bad(message: impl Into<String>) -> ApiError {
    ApiError::bad_request(message)
}

fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

fn run_error(e: RunError) -> ApiError {
    match e {
        RunError::Refused(why) => bad(why),
        other => failed(other.to_string()),
    }
}

impl AgyOps {
    /// Nothing is started until an acting op is called with the switch on.
    pub fn new(
        acting: Arc<Acting>,
        room: Arc<dyn Room>,
        spawner: Arc<dyn Spawner>,
        setup: AgySetup,
    ) -> AgyOps {
        let shared = Arc::new(Shared {
            room,
            pending: Mutex::new(BTreeMap::new()),
            last_chunk: Mutex::new(HashMap::new()),
            started: Mutex::new(HashMap::new()),
        });
        let sink = Arc::new(DaemonSink {
            shared: shared.clone(),
            runner: std::sync::OnceLock::new(),
        });
        let runner = Runner::new(
            spawner,
            setup.binary.clone(),
            sink.clone(),
            Arc::new(now_secs),
            setup.timing,
        );
        let _ = sink.runner.set(runner.clone());
        AgyOps {
            acting,
            shared,
            runner,
            setup,
        }
    }

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            SESSIONS => Some(OpSpec::AGENT),
            STATE => Some(OpSpec::APP),
            op if ACTING_OPS.contains(&op) => Some(OpSpec::APP),
            _ => None,
        }
    }

    pub fn call(&self, op: &str, args: &Value) -> Result<Value, ApiError> {
        if ACTING_OPS.contains(&op) && !self.acting.is_on() {
            return Err(ApiError::new(ErrorCode::Forbidden, ACTING_OFF));
        }
        match op {
            SESSIONS => Ok(self.sessions()),
            STATE => self.state(args),
            OPEN => self.open(args),
            SEND => self.send(args),
            CANCEL => {
                let session = self.session_arg(args)?;
                self.runner.cancel(&session);
                Ok(json!({"cancelled": true}))
            }
            SET_OPTION => {
                let session = self.session_arg(args)?;
                let option = text_arg(args, "option")?;
                let value = text_arg(args, "value")?;
                self.runner
                    .set_option(&session, option, value)
                    .map_err(run_error)?;
                Ok(json!({"requested": true}))
            }
            PERMISSION => self.permission(args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    /// The permissions waiting, as `prompts.list` rows.
    pub fn prompts(&self) -> Vec<Value> {
        lock(&self.shared.pending).values().cloned().collect()
    }

    /// Acting was turned off: no `agy` stays running, and what waited is cancelled.
    pub fn acting_off(&self) {
        self.runner.close_all();
        self.drain_pending("acting_off");
    }

    /// The daemon is stopping.
    pub fn close(&self) {
        self.runner.close_all();
        self.drain_pending("closed");
    }

    /// Closes sessions idle past ten minutes; the daemon calls it every so often.
    pub fn reap(&self) {
        self.runner.reap();
    }

    /// Permissions still listed when nothing can answer them any more.
    fn drain_pending(&self, by: &str) {
        let left = std::mem::take(&mut *lock(&self.shared.pending));
        for (id, data) in left {
            let data = json!({"id": id, "sessionId": data["sessionId"], "tool": "agy",
                              "outcome": "cancelled", "by": by});
            self.shared
                .publish_both(PERMISSION_RESOLVED, topics::PROMPT_RESOLVED, &data);
        }
    }

    // MARK: Ops

    fn rows(&self) -> Vec<Row> {
        summaries::rows(&self.setup.root, &self.setup.copy_dir, now_secs())
    }

    fn sessions(&self) -> Value {
        let working = self.runner.working();
        let mut out: Vec<Value> = Vec::new();
        let mut listed: Vec<String> = Vec::new();
        for row in self.rows() {
            listed.push(row.id.clone());
            let name = self.runner.title(&row.id).unwrap_or(row.title.clone());
            out.push(session_json(
                &row.row_id(),
                &name,
                &row.folder,
                row.modified,
                working.contains(&row.id),
            ));
        }
        for (id, s) in lock(&self.shared.started).iter() {
            if !listed.contains(id) && self.runner.is_open(id) {
                let name = self.runner.title(id).unwrap_or(s.name.clone());
                out.push(session_json(
                    &row_id(id),
                    &name,
                    &s.folder,
                    s.at,
                    working.contains(id),
                ));
            }
        }
        out.sort_by(|a, b| since(b).total_cmp(&since(a)));
        Value::Array(out)
    }

    fn state(&self, args: &Value) -> Result<Value, ApiError> {
        let session = self.session_arg(args)?;
        let r = &self.runner;
        Ok(json!({
            "id": row_id(&session),
            "open": r.is_open(&session),
            "loading": r.is_loading(&session),
            "loadError": r.load_error(&session),
            "working": r.is_working(&session),
            "title": r.title(&session),
            "folder": r.folder(&session),
            "config": r.config(&session).iter().map(|o| o.to_json()).collect::<Vec<_>>(),
            "commands": r.commands(&session).iter().map(|c| c.to_json()).collect::<Vec<_>>(),
            "permission": r.permission(&session).map(|p| p.to_json()),
        }))
    }

    fn open(&self, args: &Value) -> Result<Value, ApiError> {
        let session = self.session_arg(args)?;
        let folder = self.folder_for(&session)?;
        let opened = self.runner.open(&session, &folder, false);
        self.settle();
        opened.map_err(run_error)?;
        Ok(json!({"id": row_id(&session), "loading": true}))
    }

    fn send(&self, args: &Value) -> Result<Value, ApiError> {
        let text = text_arg(args, "text")?;
        if text.trim().is_empty() {
            return Err(bad("the message is empty"));
        }
        if args.get("id").is_some() {
            let session = self.session_arg(args)?;
            let folder = self.folder_for(&session)?;
            let sent = self.runner.prompt(&session, text, &folder);
            self.settle();
            sent.map_err(run_error)?;
            return Ok(json!({"id": row_id(&session), "sent": true}));
        }
        let folder = text_arg(args, "draft_folder")
            .map_err(|_| bad("id or draft_folder is needed"))?
            .to_string();
        let path = Path::new(&folder);
        if !path.is_absolute() || !path.is_dir() {
            return Err(bad(
                "draft_folder is an absolute path to a folder that exists",
            ));
        }
        let started = self.runner.start(&folder, self.setup.start_wait);
        self.settle();
        let session = started.map_err(run_error)?;
        lock(&self.shared.started).insert(
            session.clone(),
            Started {
                name: summaries::title(text),
                folder: folder.clone(),
                at: now_secs(),
            },
        );
        let sent = self.runner.prompt(&session, text, &folder);
        self.settle();
        sent.map_err(run_error)?;
        Ok(json!({"id": row_id(&session), "sent": true}))
    }

    /// Acting may have been turned off while an op was starting a process: nothing it started
    /// stays.
    fn settle(&self) {
        if !self.acting.is_on() {
            self.acting_off();
        }
    }

    fn permission(&self, args: &Value) -> Result<Value, ApiError> {
        let session = self.session_arg(args)?;
        let rpc = text_arg(args, "rpc_id")?;
        let option = match args.get("option_id") {
            None | Some(Value::Null) => None,
            Some(Value::String(o)) => Some(o.as_str()),
            Some(_) => return Err(bad("option_id is text")),
        };
        let outcome = self
            .runner
            .answer(&session, rpc, option)
            .map_err(run_error)?;
        Ok(json!({"outcome": outcome}))
    }

    /// The `agy:<session>` row id's session.
    fn session_arg(&self, args: &Value) -> Result<String, ApiError> {
        let id = text_arg(args, "id")?;
        id.strip_prefix("agy:")
            .filter(|s| !s.is_empty() && !s.contains(['/', '\\']))
            .map(String::from)
            .ok_or_else(|| bad("id is an Antigravity row id, agy:<session>"))
    }

    /// Where a session runs: the one this daemon holds, else the summaries copy's. A session
    /// neither knows is refused, so a caller can't point `agy` at a folder or a made-up id.
    fn folder_for(&self, session: &str) -> Result<String, ApiError> {
        if let Some(folder) = self.runner.folder(session) {
            return Ok(folder);
        }
        self.rows()
            .into_iter()
            .find(|r| r.id == session)
            .map(|r| r.folder)
            .ok_or_else(|| bad(format!("no Antigravity session {session}")))
    }
}

fn since(row: &Value) -> f64 {
    row["sinceMs"].as_f64().unwrap_or(0.0)
}

fn session_json(id: &str, name: &str, folder: &str, modified: f64, working: bool) -> Value {
    json!({
        "id": id,
        "name": name,
        "project": Path::new(folder).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "cwd": folder,
        "sinceMs": modified * 1000.0,
        "working": working,
    })
}

/// `agy` and its list: `root` is `~/.gemini/antigravity-cli`, `copy_dir` our own folder. Over a
/// fixture root only `<home>/.local/bin/agy` counts, never the machine's own install.
pub fn setup_for(root: PathBuf, copy_dir: PathBuf, home: &Path, fixture: bool) -> AgySetup {
    let binary = if fixture {
        let own = home.join(".local/bin/agy");
        own.is_file().then_some(own)
    } else {
        agy::find_binary(home)
    };
    AgySetup {
        root,
        copy_dir,
        binary: binary.map(|p| p.to_string_lossy().into_owned()),
        start_wait: Duration::from_secs(4),
        timing: Timing::default(),
    }
}

#[cfg(test)]
#[path = "agy_ops_tests.rs"]
mod tests;
