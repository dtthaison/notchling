use crate::acting::{Acting, ACTING_OFF};
use crate::acting_ops::{broad_folder, text_arg, Room};
use crate::agy_elsewhere::Elsewhere;
use crate::agy_sink::{lock, now_secs, row_id, DaemonSink, Shared, Started};
use crate::catalog_ops::CatalogService;
use crate::layout::Layout;
use crate::pages_ops::Titles;
use notchling_engine::search::index::TitledSession;
use notchling_engine::tools::acp::{RealSpawner, Spawner};
use notchling_engine::tools::agy::summaries::{self, Row};
use notchling_engine::tools::agy::{self, RunError, Runner, Timing};
use notchling_engine::tools::codex::plan::MAX_PROMPT_BYTES;
use notchling_local_api::{ops, topics, ApiError, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const COPY_REUSE: Duration = Duration::from_secs(3);

pub const SESSIONS: &str = ops::AGY_SESSIONS;
pub const STATE: &str = ops::AGY_STATE;
pub const CONVERSATION: &str = ops::AGY_CONVERSATION;
pub const OPEN: &str = ops::AGY_OPEN;
pub const SEND: &str = ops::AGY_SEND;
pub const CANCEL: &str = ops::AGY_CANCEL;
pub const SET_OPTION: &str = ops::AGY_SET_OPTION;
pub const PERMISSION: &str = ops::AGY_PERMISSION;

pub const PERMISSION_PENDING: &str = topics::AGY_PERMISSION_PENDING;
pub const PERMISSION_RESOLVED: &str = topics::AGY_PERMISSION_RESOLVED;

const CONVERSATION_BYTES: usize = 512 * 1024;

pub const ACTING_OPS: [&str; 5] = [OPEN, SEND, CANCEL, SET_OPTION, PERMISSION];

pub const READ_OPS: [&str; 2] = [STATE, CONVERSATION];

#[derive(Clone, Debug)]
pub struct AgySetup {
    pub root: PathBuf,
    pub copy_dir: PathBuf,
    pub home: PathBuf,
    pub binary: Option<String>,
    pub start_wait: Duration,
    pub timing: Timing,
}

pub struct AgyOps {
    acting: Arc<Acting>,
    shared: Arc<Shared>,
    runner: Runner,
    setup: AgySetup,
    elsewhere: Arc<Elsewhere>,
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
        RunError::NotAllowed => ApiError::new(ErrorCode::Forbidden, ACTING_OFF),
        other => failed(other.to_string()),
    }
}

impl AgyOps {
    pub fn new(
        acting: Arc<Acting>,
        room: Arc<dyn Room>,
        spawner: Arc<dyn Spawner>,
        setup: AgySetup,
    ) -> AgyOps {
        let shared = Arc::new(Shared::new(room));
        let switch = acting.clone();
        let runner = Runner::new(
            spawner,
            setup.binary.clone(),
            Arc::new(DaemonSink {
                shared: shared.clone(),
            }),
            Arc::new(now_secs),
            setup.timing,
        );
        runner.set_may_start(Arc::new(move || switch.is_on()));
        let _ = shared.runner.set(runner.clone());
        let elsewhere = Arc::new(Elsewhere::new(
            setup.root.clone(),
            setup.home.clone(),
            runner.clone(),
            shared.clone(),
        ));
        AgyOps {
            acting,
            shared,
            runner,
            setup,
            elsewhere,
        }
    }

    pub fn attach_catalog(&self, catalog: Arc<CatalogService>) {
        let _ = self.shared.catalog.set(catalog);
    }

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            SESSIONS => Some(OpSpec::AGENT),
            op if READ_OPS.contains(&op) || ACTING_OPS.contains(&op) => Some(OpSpec::APP),
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
            CONVERSATION => self.conversation(args),
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

    #[cfg(test)]
    pub(crate) fn elsewhere(&self) -> &Arc<Elsewhere> {
        &self.elsewhere
    }

    pub fn titles(self: &Arc<Self>) -> Titles {
        let ops = self.clone();
        Arc::new(move || ops.titled_sessions())
    }

    fn titled_sessions(&self) -> Vec<TitledSession> {
        self.rows()
            .into_iter()
            .map(|r| TitledSession {
                id: r.id,
                title: r.title,
                folder: r.folder,
                modified: r.modified,
            })
            .collect()
    }

    pub fn prompts(&self) -> Vec<Value> {
        lock(&self.shared.pending).values().cloned().collect()
    }

    pub fn acting_off(&self) {
        self.runner.close_all();
        self.drain_pending("acting_off");
    }

    pub fn close(&self) {
        self.runner.close_all();
        self.drain_pending("closed");
    }

    pub fn reap(&self) {
        self.runner.reap();
    }

    pub fn working_count(&self) -> usize {
        self.runner.working().len()
    }

    fn drain_pending(&self, by: &str) {
        let left = std::mem::take(&mut *lock(&self.shared.pending));
        for (id, data) in left {
            let data = json!({"id": id, "sessionId": data["sessionId"], "tool": "agy",
                              "outcome": "cancelled", "by": by});
            self.shared
                .publish_both(PERMISSION_RESOLVED, topics::PROMPT_RESOLVED, &data);
        }
    }

    fn rows(&self) -> Vec<Row> {
        summaries::rows_cached(
            &self.setup.root,
            &self.setup.copy_dir,
            now_secs(),
            COPY_REUSE,
        )
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
        let elsewhere = self.elsewhere.watched(&session);
        let working = match &elsewhere {
            Some(holder) => self.elsewhere.conversation(&session, holder).1,
            None => r.is_working(&session),
        };
        Ok(json!({
            "id": row_id(&session),
            "open": r.is_open(&session),
            "loading": r.is_loading(&session),
            "loadError": r.load_error(&session),
            "working": working,
            "elsewhere": elsewhere.map(|h| h.note()),
            "title": r.title(&session),
            "folder": r.folder(&session),
            "config": r.config(&session).iter().map(|o| o.to_json()).collect::<Vec<_>>(),
            "commands": r.commands(&session).iter().map(|c| c.to_json()).collect::<Vec<_>>(),
            "permission": r.permission(&session).map(|p| p.to_json()),
        }))
    }

    fn conversation(&self, args: &Value) -> Result<Value, ApiError> {
        let session = self.session_arg(args)?;
        let peek = match args.get("peek") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(peek)) => *peek,
            Some(_) => return Err(bad("peek is true or false")),
        };
        let r = &self.runner;
        if let Some(holder) = self.elsewhere.watched(&session) {
            let (held, working) = self.elsewhere.conversation(&session, &holder);
            let (held, omitted) = held.bounded(CONVERSATION_BYTES);
            let conversation = serde_json::to_value(held).map_err(|e| failed(e.to_string()))?;
            return Ok(json!({
                "id": row_id(&session),
                "open": false,
                "loading": false,
                "loadError": null,
                "working": working,
                "conversation": conversation,
                "truncated": omitted > 0,
                "omitted": omitted,
                "elsewhere": holder.note(),
            }));
        }
        let held = if peek {
            r.peek_conversation(&session)
        } else {
            r.conversation(&session)
        };
        let open = held.is_some();
        let (held, omitted) = held.unwrap_or_default().bounded(CONVERSATION_BYTES);
        let conversation = serde_json::to_value(held).map_err(|e| failed(e.to_string()))?;
        Ok(json!({
            "id": row_id(&session),
            "open": open,
            "loading": r.is_loading(&session),
            "loadError": r.load_error(&session),
            "working": r.is_working(&session),
            "conversation": conversation,
            "truncated": omitted > 0,
            "omitted": omitted,
        }))
    }

    fn open(&self, args: &Value) -> Result<Value, ApiError> {
        let session = self.session_arg(args)?;
        let folder = self.folder_for(&session)?;
        if let Some(holder) = self.elsewhere.check(&session) {
            return Ok(json!({"id": row_id(&session), "loading": false,
                             "elsewhere": holder.note()}));
        }
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
        if text.len() > MAX_PROMPT_BYTES {
            return Err(bad(format!(
                "the message is over {} KB",
                MAX_PROMPT_BYTES / 1024
            )));
        }
        if args.get("id").is_some() {
            let session = self.session_arg(args)?;
            let folder = self.folder_for(&session)?;
            if let Some(holder) = self.elsewhere.check(&session) {
                return Err(failed(holder.note()));
            }
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
        let real = std::fs::canonicalize(path)
            .map_err(|_| bad("draft_folder is an absolute path to a folder that exists"))?;
        if broad_folder(&real, &self.setup.home) {
            return Err(bad(
                "a new session never starts in the root or the home folder",
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

    fn session_arg(&self, args: &Value) -> Result<String, ApiError> {
        let id = text_arg(args, "id")?;
        id.strip_prefix("agy:")
            .filter(|s| !s.is_empty() && !s.contains(['/', '\\']))
            .map(String::from)
            .ok_or_else(|| bad("id is an Antigravity row id, agy:<session>"))
    }

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

impl AgyOps {
    pub fn for_daemon(layout: &Layout, acting: Arc<Acting>, room: Arc<dyn Room>) -> AgyOps {
        let home = layout.home();
        let binary = if layout.fixture {
            let own = home.join(".local/bin/agy");
            own.is_file().then_some(own)
        } else {
            agy::find_binary(&home)
        };
        AgyOps::new(
            acting,
            room,
            Arc::new(RealSpawner::new(&home)),
            AgySetup {
                root: layout.antigravity_dir(),
                copy_dir: layout.antigravity_copy_dir(),
                home,
                binary: binary.map(|p| p.to_string_lossy().into_owned()),
                start_wait: Duration::from_secs(4),
                timing: Timing::default(),
            },
        )
    }
}

#[cfg(test)]
#[path = "agy_ops_tests.rs"]
mod tests;
