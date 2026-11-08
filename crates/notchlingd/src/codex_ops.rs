//! Milestone 5's Codex ops (E6a): `codex.send`, `codex.stop` and `codex.delete` over the engine's
//! [`CodexRuns`]. App tier, audit-logged by the server like every app-tier op, and refused with
//! [`ACTING_OFF`] while the `acting` switch is off: every one of them starts `codex` or sends it a
//! signal.
//!
//! | Op | Args | Reply |
//! | --- | --- | --- |
//! | `codex.send` | `{id \| draft_folder, text, model?, effort?}` | `{"id": "codex:<session>", "session", "state": "started"}`, or `"state": "queued", "waiting": n` behind the session's running turn |
//! | `codex.stop` | `{id}` | `{"stopped": bool}`: SIGINT to the running turn; false when none runs |
//! | `codex.delete` | `{id}` | `{"deleted": true}`: stops the turn, drops what waits, runs `codex delete --force` |
//!
//! `id` is a row id (`codex:<session>`). `draft_folder` starts a new session in that folder and
//! the reply comes once `thread.started` names it. Events, on topic `tools`: `codex.turn {id,
//! session, state: started | queued | ended | failed, waiting?, error?}` and `session.changed
//! {id, session, row}` ([`topics`]).
//!
//! What a caller may name is narrow, since whoever holds the app tier for a moment shouldn't be
//! able to point a sandboxed-but-writable agent anywhere or pass Codex options:
//!
//! - a session must have a rollout under the Codex home (`delete` and `send` to a session that
//!   isn't running), and runs in the folder the rollout records, which must still exist;
//! - `draft_folder` is an absolute path to an existing folder that is neither `/` nor the home
//!   folder (the workspace-write sandbox makes the folder writable);
//! - `model` is a slug (letters, digits, `.`, `_`, `-`), `effort` a word, and an effort the
//!   model's cache entry doesn't list is refused; nothing else reaches Codex's command line, and a
//!   prompt that starts with `-` follows a `--`;
//! - a prompt is at most 100 KB, and at most eight turns run at once.

use crate::acting::{Acting, ACTING_OFF};
use crate::acting_ops::{bad, forbidden, text_arg, Room};
use crate::layout::Layout;
use notchling_engine::control::{Command, Launcher, RealLauncher};
use notchling_engine::sessions::AgentGroup;
use notchling_engine::tools::codex::list::{find_rollout, session as row_of};
use notchling_engine::tools::codex::models;
use notchling_engine::tools::codex::plan::{
    augmented_path, find_binary, valid_effort, valid_model, valid_session_id, Pick,
    MAX_PROMPT_BYTES,
};
use notchling_engine::tools::codex::process::{RealSpawner, ToolSpawner};
use notchling_engine::tools::codex::runs::{CodexRuns, Request, Sent, TurnEvent, TurnSink};
use notchling_engine::transcript::codex::meta;
use notchling_local_api::{ops, topics, ApiError, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Every Codex op; all app tier, all acting.
pub const CODEX_OPS: [&str; 3] = [ops::CODEX_SEND, ops::CODEX_STOP, ops::CODEX_DELETE];

/// A row id's prefix (`CodexSessions.agentID`).
const ID_PREFIX: &str = "codex:";

/// How long `codex delete` may run, and how long a stopped turn is waited for first.
const DELETE_TIMEOUT: Duration = Duration::from_secs(60);
const STOP_SETTLE: Duration = Duration::from_secs(3);

/// Where Codex's files and ours are.
#[derive(Clone, Debug)]
pub struct CodexSetup {
    /// `CODEX_HOME`, else `~/.codex`: `sessions/`, `models_cache.json`.
    pub codex_home: PathBuf,
    pub home: PathBuf,
    /// Notchling's data folder, for `settings.json` (`codexNetwork`).
    pub data_dir: PathBuf,
}

/// The `codex` command and the PATH its tools are found in; `None` when Codex isn't installed.
pub type Resolver = Box<dyn Fn() -> Option<(PathBuf, String)> + Send + Sync>;

pub struct CodexOps {
    acting: Arc<Acting>,
    runs: CodexRuns,
    launcher: Arc<dyn Launcher>,
    setup: CodexSetup,
    resolve: Resolver,
    room: Arc<dyn Room>,
}

impl CodexOps {
    pub fn new(
        acting: Arc<Acting>,
        setup: CodexSetup,
        spawner: Arc<dyn ToolSpawner>,
        launcher: Arc<dyn Launcher>,
        room: Arc<dyn Room>,
    ) -> CodexOps {
        let sink = Arc::new(RoomSink {
            room: room.clone(),
            sessions_dir: setup.codex_home.join("sessions"),
        });
        let home = setup.home.clone();
        CodexOps {
            acting,
            runs: CodexRuns::new(spawner, PathBuf::new(), None, sink),
            launcher,
            setup,
            resolve: Box::new(move || system_codex(&home)),
            room,
        }
    }

    /// The daemon's Codex ops: the real `codex` processes, `codex delete` through the real launcher.
    /// `claude` is only the launcher's path to Claude Code, which these ops never run.
    pub fn for_daemon(
        layout: &Layout,
        acting: Arc<Acting>,
        room: Arc<dyn Room>,
        claude: &Path,
    ) -> CodexOps {
        CodexOps::new(
            acting,
            CodexSetup {
                codex_home: layout.codex_dir(),
                home: layout.home(),
                data_dir: layout.data_dir.clone(),
            },
            Arc::new(RealSpawner),
            Arc::new(RealLauncher::with_claude(claude.to_path_buf())),
            room,
        )
    }

    /// Finds `codex` with `resolve` instead of the system's PATH (tests).
    pub fn with_resolver(mut self, resolve: Resolver) -> CodexOps {
        self.resolve = resolve;
        self
    }

    pub fn runs(&self) -> &CodexRuns {
        &self.runs
    }

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        CODEX_OPS.contains(&op).then_some(OpSpec::APP)
    }

    /// The daemon is stopping: running turns are asked to stop, which Codex does cleanly, rather
    /// than left with a pipe nobody reads.
    pub fn close(&self) {
        self.runs.stop_all();
    }

    pub fn call(&self, op: &str, args: &Value) -> Result<Value, ApiError> {
        if !self.acting.is_on() {
            return Err(forbidden(ACTING_OFF));
        }
        match op {
            ops::CODEX_SEND => self.send(args),
            ops::CODEX_STOP => self.stop(args),
            ops::CODEX_DELETE => self.delete(args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn sessions_dir(&self) -> PathBuf {
        self.setup.codex_home.join("sessions")
    }

    /// The session `args.id` names (`codex:<session>`).
    fn session_arg(args: &Value) -> Result<String, ApiError> {
        let id = text_arg(args, "id")?;
        let session = id
            .strip_prefix(ID_PREFIX)
            .ok_or_else(|| bad("id is a Codex row id (codex:<session>)"))?;
        if !valid_session_id(session) {
            return Err(bad("that isn't a Codex session id"));
        }
        Ok(session.to_string())
    }

    fn send(&self, args: &Value) -> Result<Value, ApiError> {
        let text = text_arg(args, "text")?;
        if text.trim().is_empty() {
            return Err(bad("the message is empty"));
        }
        if text.len() > MAX_PROMPT_BYTES {
            return Err(bad(format!(
                "the message is over {} KB",
                MAX_PROMPT_BYTES / 1024
            )));
        }
        let pick = self.pick(args)?;
        let (session, folder) = match (args.get("id"), args.get("draft_folder")) {
            (Some(_), None) => {
                let session = Self::session_arg(args)?;
                let folder = self.folder_of(&session)?;
                (Some(session), folder)
            }
            (None, Some(_)) => (None, self.draft_folder(text_arg(args, "draft_folder")?)?),
            _ => {
                return Err(bad(
                    "pass id (a session) or draft_folder (a new one), not both",
                ))
            }
        };
        let (codex, path) = (self.resolve)().ok_or_else(|| {
            ApiError::new(
                ErrorCode::Failed,
                "Couldn't find the codex command. Install the Codex CLI first.",
            )
        })?;
        self.runs.set_command(codex, Some(path));
        self.runs.set_network(self.network_allowed());
        let sent = self
            .runs
            .send(Request {
                text: text.to_string(),
                session,
                folder,
                pick,
            })
            .map_err(|e| ApiError::new(ErrorCode::Failed, e.to_string()))?;
        Ok(match sent {
            Sent::Started { session } => {
                json!({"id": row_id(&session), "session": session, "state": "started"})
            }
            Sent::Queued { session, waiting } => json!({
                "id": row_id(&session), "session": session, "state": "queued", "waiting": waiting
            }),
        })
    }

    fn stop(&self, args: &Value) -> Result<Value, ApiError> {
        let session = Self::session_arg(args)?;
        Ok(json!({"stopped": self.runs.stop(&session)}))
    }

    fn delete(&self, args: &Value) -> Result<Value, ApiError> {
        let session = Self::session_arg(args)?;
        if find_rollout(&self.sessions_dir(), &session).is_none() {
            return Err(bad(format!("no Codex session {session}")));
        }
        let (codex, path) = (self.resolve)()
            .ok_or_else(|| ApiError::new(ErrorCode::Failed, "Couldn't find the codex command."))?;
        // The turn is stopped and its waiting messages dropped first, and given a moment to let go
        // of the file `codex delete` removes.
        if self.runs.forget(&session) {
            let until = Instant::now() + STOP_SETTLE;
            while self.runs.is_running(&session) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let command = Command::from_args(
            &codex.to_string_lossy(),
            &["delete".into(), "--force".into(), session.clone()],
        )
        .with_env("PATH", Some(&path));
        let done = self
            .launcher
            .run(&command, DELETE_TIMEOUT)
            .map_err(|e| ApiError::new(ErrorCode::Failed, e.to_string()))?;
        if done.code != 0 {
            let line = done.stderr.lines().rev().find(|l| !l.trim().is_empty());
            return Err(ApiError::new(
                ErrorCode::Failed,
                line.map_or_else(
                    || format!("codex delete exited with code {}.", done.code),
                    |l| l.trim().to_string(),
                ),
            ));
        }
        self.room.publish(
            topics::TOOLS,
            topics::SESSION_CHANGED,
            &json!({"id": row_id(&session), "session": session, "row": Value::Null, "deleted": true}),
        );
        Ok(json!({"deleted": true}))
    }

    /// The folder a session runs in: the one its rollout records. A session whose turn is running
    /// needs none (its message waits).
    fn folder_of(&self, session: &str) -> Result<String, ApiError> {
        if self.runs.is_running(session) {
            return Ok(String::new());
        }
        let rollout = find_rollout(&self.sessions_dir(), session)
            .ok_or_else(|| bad(format!("no Codex session {session}")))?;
        let cwd = meta(&rollout.to_string_lossy())
            .map(|m| m.cwd)
            .filter(|c| !c.is_empty())
            .ok_or_else(|| bad("that session's file names no folder"))?;
        if !Path::new(&cwd).is_absolute() || !Path::new(&cwd).is_dir() {
            return Err(bad(format!("the session's folder {cwd} is gone")));
        }
        Ok(cwd)
    }

    /// The folder a new session may start in: see the module's rules.
    fn draft_folder(&self, folder: &str) -> Result<String, ApiError> {
        let path = Path::new(folder);
        if !path.is_absolute() {
            return Err(bad("draft_folder is an absolute path"));
        }
        let real =
            std::fs::canonicalize(path).map_err(|_| bad(format!("{folder} doesn't exist")))?;
        if !real.is_dir() {
            return Err(bad(format!("{folder} isn't a folder")));
        }
        let home = std::fs::canonicalize(&self.setup.home).unwrap_or(self.setup.home.clone());
        if real.parent().is_none() || real == home {
            return Err(bad(
                "a new session never starts in the root or the home folder",
            ));
        }
        Ok(real.to_string_lossy().into_owned())
    }

    fn pick(&self, args: &Value) -> Result<Pick, ApiError> {
        let word = |key: &str| -> Result<Option<String>, ApiError> {
            match args.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.clone())),
                Some(_) => Err(bad(format!("{key} is a string"))),
            }
        };
        let (model, effort) = (word("model")?, word("effort")?);
        if model.as_deref().is_some_and(|m| !valid_model(m)) {
            return Err(bad("model is a model name like gpt-6.1-sol"));
        }
        if effort.as_deref().is_some_and(|e| !valid_effort(e)) {
            return Err(bad("effort is a level like low, medium, high or xhigh"));
        }
        if let (Some(model), Some(effort)) = (&model, &effort) {
            let known = models::load(&self.setup.codex_home);
            if !models::supports(&known, model, effort) {
                return Err(bad(format!("{model} doesn't offer the effort {effort}")));
            }
        }
        Ok(Pick { model, effort })
    }

    /// "Allow Network in Codex Turns" (`codexNetwork`) from the data folder's settings; on unless
    /// the file says off.
    fn network_allowed(&self) -> bool {
        notchling_engine::settings::Settings::open(&self.setup.data_dir.join("settings.json"))
            .ok()
            .and_then(|s| s.get_bool("codexNetwork"))
            .unwrap_or(true)
    }
}

fn row_id(session: &str) -> String {
    format!("{ID_PREFIX}{session}")
}

/// `codex` from the usual places or the process's PATH, and the PATH it runs with.
fn system_codex(home: &Path) -> Option<(PathBuf, String)> {
    let path = std::env::var("PATH").unwrap_or_default();
    let codex = find_binary(home, &path, Path::is_file)?;
    Some((codex, augmented_path(&path, home)))
}

/// Publishes a turn's events and the row it changes.
struct RoomSink {
    room: Arc<dyn Room>,
    sessions_dir: PathBuf,
}

impl RoomSink {
    /// The session's row as `sessions.list` would give it, with the runner's word for what it is
    /// doing: Working while a turn runs, Idle after (a turn the file still shows open was stopped).
    fn row(&self, session: &str, working: bool) -> Value {
        let Some(mut agent) = find_rollout(&self.sessions_dir, session).and_then(|p| row_of(&p))
        else {
            return Value::Null;
        };
        (agent.group, agent.label) = if working {
            (AgentGroup::Working, "Working".to_string())
        } else {
            (AgentGroup::Completed, "Idle".to_string())
        };
        serde_json::to_value(agent).unwrap_or(Value::Null)
    }

    fn changed(&self, session: &str, working: bool) {
        self.room.publish(
            topics::TOOLS,
            topics::SESSION_CHANGED,
            &json!({"id": row_id(session), "session": session, "row": self.row(session, working)}),
        );
    }

    fn turn(&self, data: &Value) {
        self.room.publish(topics::TOOLS, topics::CODEX_TURN, data);
    }
}

impl TurnSink for RoomSink {
    fn event(&self, event: TurnEvent) {
        match event {
            TurnEvent::Started { session } => {
                self.turn(&json!({"id": row_id(&session), "session": session, "state": "started"}));
                self.changed(&session, true);
            }
            TurnEvent::Queued { session, waiting } => {
                self.turn(&json!({
                    "id": row_id(&session), "session": session, "state": "queued", "waiting": waiting
                }));
            }
            TurnEvent::Ended { session, failure } => {
                let state = if failure.is_some() { "failed" } else { "ended" };
                let mut data = json!({
                    "id": session.as_deref().map(row_id), "session": session, "state": state
                });
                if let Some(error) = failure {
                    data["error"] = json!(error);
                }
                self.turn(&data);
                if let Some(session) = session {
                    self.changed(&session, false);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "codex_ops_tests.rs"]
mod tests;
