use crate::acting::{Acting, ACTING_OFF};
use crate::acting_ops::{bad, broad_folder, forbidden, text_arg, Room};
use crate::layout::Layout;
use notchling_engine::control::{Command, Launcher, RealLauncher};
use notchling_engine::sessions::AgentGroup;
use notchling_engine::tools::codex::list::{find_rollout, session as row_of};
use notchling_engine::tools::codex::models;
use notchling_engine::tools::codex::plan::{
    augmented_path, find_binary, find_fixture_binary, valid_effort, valid_model, valid_session_id,
    Pick, MAX_PROMPT_BYTES,
};
use notchling_engine::tools::codex::process::{RealSpawner, ToolSpawner};
use notchling_engine::tools::codex::runs::{
    CodexRuns, Request, SendError, Sent, TurnEvent, TurnSink,
};
use notchling_engine::transcript::codex::meta;
use notchling_local_api::{ops, topics, ApiError, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const CODEX_OPS: [&str; 3] = [ops::CODEX_SEND, ops::CODEX_STOP, ops::CODEX_DELETE];

const ID_PREFIX: &str = "codex:";

const REPLY_WAIT: Duration = Duration::from_millis(4_500);

const DELETE_TIMEOUT: Duration = Duration::from_secs(60);
const STOP_SETTLE: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub struct CodexSetup {
    pub codex_home: PathBuf,
    pub home: PathBuf,
    pub data_dir: PathBuf,
    pub fixture: bool,
}

pub type Resolver = Box<dyn Fn() -> Option<(PathBuf, String)> + Send + Sync>;

pub struct CodexOps {
    acting: Arc<Acting>,
    runs: CodexRuns,
    launcher: Arc<dyn Launcher>,
    setup: CodexSetup,
    resolve: Resolver,
    room: Arc<dyn Room>,
    reply_wait_ms: AtomicU64,
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
        let found = setup.clone();
        let runs = CodexRuns::new(spawner, PathBuf::new(), None, sink);
        let switch = acting.clone();
        runs.set_may_start(Arc::new(move || switch.is_on()));
        runs.set_codex_home(setup.codex_home.clone());
        CodexOps {
            acting,
            runs,
            launcher,
            setup,
            resolve: Box::new(move || system_codex(&found)),
            room,
            reply_wait_ms: AtomicU64::new(REPLY_WAIT.as_millis() as u64),
        }
    }

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
                fixture: layout.fixture,
            },
            Arc::new(RealSpawner),
            Arc::new(RealLauncher::with_claude(claude.to_path_buf())),
            room,
        )
    }

    pub fn with_resolver(mut self, resolve: Resolver) -> CodexOps {
        self.resolve = resolve;
        self
    }

    pub fn set_reply_wait(&self, wait: Duration) {
        let ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
        self.reply_wait_ms.store(ms, Ordering::SeqCst);
    }

    pub fn runs(&self) -> &CodexRuns {
        &self.runs
    }

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        CODEX_OPS.contains(&op).then_some(OpSpec::APP)
    }

    pub fn close(&self) {
        self.runs.stop_all();
    }

    pub fn acting_off(&self) {
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
        let draft = Self::draft_arg(args, args.get("draft_folder").is_some())?;
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
        let request = Request {
            text: text.to_string(),
            session,
            folder,
            pick,
        };
        if request.session.is_none() {
            return self.send_new(request, draft);
        }
        self.runs.send(request).map(sent_json).map_err(send_error)
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
        self.delete_session(session, codex, path)
    }

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
        if broad_folder(&real, &self.setup.home) {
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

fn system_codex(setup: &CodexSetup) -> Option<(PathBuf, String)> {
    let path = std::env::var("PATH").unwrap_or_default();
    locate(setup, &path, Path::is_file)
}

fn locate(
    setup: &CodexSetup,
    path: &str,
    is_file: impl Fn(&Path) -> bool,
) -> Option<(PathBuf, String)> {
    let codex = if setup.fixture {
        find_fixture_binary(&setup.home, is_file)?
    } else {
        find_binary(&setup.home, path, is_file)?
    };
    Some((codex, augmented_path(path, &setup.home)))
}

struct RoomSink {
    room: Arc<dyn Room>,
    sessions_dir: PathBuf,
}

impl RoomSink {
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

#[path = "codex_slow.rs"]
mod slow;
use slow::{send_error, sent_json};

#[cfg(test)]
#[path = "codex_ops_tests.rs"]
mod tests;
