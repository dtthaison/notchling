use crate::acting_ops::{now_ms, Room};
use crate::catalog_ops::CatalogService;
use notchling_engine::bridge::foreign::PROMPT_PREFIX;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::tools::agy::{Change, Permission, Runner, Sink};
use notchling_local_api::topics;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CHUNK_EVERY: Duration = Duration::from_millis(250);

pub(crate) struct Started {
    pub name: String,
    pub folder: String,
    pub at: f64,
}

pub(crate) struct Shared {
    pub room: Arc<dyn Room>,
    pub pending: Mutex<BTreeMap<String, Value>>,
    pub last_chunk: Mutex<HashMap<String, Instant>>,
    pub started: Mutex<HashMap<String, Started>>,
    pub runner: OnceLock<Runner>,
    pub catalog: OnceLock<Arc<CatalogService>>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn row_id(session: &str) -> String {
    format!("agy:{session}")
}

pub(crate) fn prompt_id(session: &str, rpc: &str) -> String {
    format!("{PROMPT_PREFIX}{}:{rpc}", row_id(session))
}

pub(crate) fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

impl Shared {
    pub fn new(room: Arc<dyn Room>) -> Shared {
        Shared {
            room,
            pending: Mutex::new(BTreeMap::new()),
            last_chunk: Mutex::new(HashMap::new()),
            started: Mutex::new(HashMap::new()),
            runner: OnceLock::new(),
            catalog: OnceLock::new(),
        }
    }

    pub fn publish_both(&self, event: &str, prompt_event: &str, data: &Value) {
        self.room.publish(topics::PROMPTS, event, data);
        self.room.publish(topics::PROMPTS, prompt_event, data);
    }

    fn options_seen(&self, session: &str) {
        if let (Some(catalog), Some(runner)) = (self.catalog.get(), self.runner.get()) {
            catalog.record_agy_options(&runner.config(session));
        }
    }

    pub fn session_changed(&self, session: &str, change: &str, error: Option<String>) {
        let mut data = json!({"id": row_id(session), "session": session,
                              "row": self.row(session), "change": change});
        if let Some(error) = error {
            data["error"] = json!(error);
        }
        self.room
            .publish(topics::TOOLS, topics::SESSION_CHANGED, &data);
    }

    pub fn row(&self, session: &str) -> Value {
        let Some(runner) = self.runner.get() else {
            return Value::Null;
        };
        let Some(folder) = runner.folder(session) else {
            return Value::Null;
        };
        let working = runner.is_working(session);
        let name = runner
            .title(session)
            .or_else(|| lock(&self.started).get(session).map(|s| s.name.clone()));
        let agent = Agent {
            id: row_id(session),
            name: name.unwrap_or_else(|| "Antigravity Session".to_string()),
            project: Path::new(&folder)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            cwd: folder.clone(),
            group: if working {
                AgentGroup::Working
            } else {
                AgentGroup::Completed
            },
            label: if working { "Working" } else { "Antigravity" }.to_string(),
            detail: None,
            since_ms: (now_secs() * 1000.0) as i64,
            target: AgentTarget::Resume {
                session_id: session.to_string(),
                cwd: folder,
            },
            suggested_reply: None,
            transcript_path: None,
            host: "Antigravity".to_string(),
            headless: false,
            config_dir: None,
            command: None,
            resume_session_id: None,
            at_prompt: false,
        };
        serde_json::to_value(agent).unwrap_or(Value::Null)
    }

    pub fn prompt_json(&self, session: &str, p: &Permission, folder: &str) -> Value {
        let card = p.to_json();
        let summary = match card["title"].as_str() {
            Some(title) if !title.is_empty() => card["title"].clone(),
            _ => card["toolName"].clone(),
        };
        let mut data = json!({
            "id": prompt_id(session, &p.rpc_text()),
            "sessionId": row_id(session),
            "tool": "agy",
            "kind": "permission",
            "toolName": card["toolName"],
            "summary": summary,
            "input": card["input"],
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

pub(crate) struct DaemonSink {
    pub shared: Arc<Shared>,
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
            Change::Loaded => {
                shared.options_seen(session);
                shared.session_changed(session, "loaded", None);
            }
            Change::LoadFailed(e) => shared.session_changed(session, "load_failed", Some(e)),
            Change::TurnStarted => shared.session_changed(session, "turn_started", None),
            Change::TurnEnded(error) => shared.session_changed(session, "turn_ended", error),
            Change::Options => {
                shared.options_seen(session);
                shared.session_changed(session, "options", None);
            }
            Change::OptionFailed(e) => shared.session_changed(session, "option_failed", Some(e)),
            Change::Closed => {
                lock(&shared.last_chunk).remove(session);
                shared.session_changed(session, "closed", None);
            }
            Change::PermissionPending(p) => {
                let folder = shared
                    .runner
                    .get()
                    .and_then(|r| r.folder(session))
                    .unwrap_or_default();
                let data = shared.prompt_json(session, &p, &folder);
                let id = data["id"].as_str().unwrap_or_default().to_string();
                lock(&shared.pending).insert(id, data.clone());
                shared.publish_both(
                    topics::AGY_PERMISSION_PENDING,
                    topics::PROMPT_PENDING,
                    &data,
                );
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
                shared.publish_both(
                    topics::AGY_PERMISSION_RESOLVED,
                    topics::PROMPT_RESOLVED,
                    &data,
                );
                shared.session_changed(session, "permission_resolved", None);
            }
        }
    }
}
