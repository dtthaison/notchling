use crate::agy_ops::AgyOps;
use crate::layout::Layout;
use notchling_engine::bridge::foreign::{self, ForeignRow, Tool};
use notchling_engine::bridge::live::ForeignLive;
use notchling_engine::contract::flows::delegate_jobs;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::tools::agy::summaries::{self, Row};
use notchling_engine::tools::codex::list as codex_list;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub const LIST_WITHIN_MS: f64 = 6.0 * 3_600_000.0;
pub const CACHE_MS: i64 = 2_000;
pub const CODEX_ROWS: usize = 30;
pub const CODEX_DAYS: i64 = 14;
const CODEX_SCAN: usize = 3 * CODEX_ROWS;
const AGY_REUSE: Duration = Duration::from_secs(3);
const HEAD_BYTES: u64 = 65_536;

pub const CLAUDE: &str = "claude";
pub const CODEX: &str = "codex";
pub const ANTIGRAVITY: &str = "antigravity";

pub struct EverySession {
    codex_sessions: PathBuf,
    within_days: Option<i64>,
    agy_root: PathBuf,
    agy_copy_dir: PathBuf,
    data_dir: PathBuf,
    cached: Mutex<Option<(i64, Vec<Agent>)>>,
    agy: OnceLock<Arc<AgyOps>>,
}

impl EverySession {
    pub fn new(layout: &Layout) -> EverySession {
        EverySession {
            codex_sessions: layout.codex_dir().join("sessions"),
            within_days: (!layout.fixture).then_some(CODEX_DAYS),
            agy_root: layout.antigravity_dir(),
            agy_copy_dir: layout.antigravity_copy_dir(),
            data_dir: layout.data_dir.clone(),
            cached: Mutex::new(None),
            agy: OnceLock::new(),
        }
    }

    pub fn none() -> EverySession {
        EverySession {
            codex_sessions: PathBuf::new(),
            within_days: None,
            agy_root: PathBuf::new(),
            agy_copy_dir: PathBuf::new(),
            data_dir: PathBuf::new(),
            cached: Mutex::new(None),
            agy: OnceLock::new(),
        }
    }

    pub fn attach_agy(&self, agy: Arc<AgyOps>) {
        let _ = self.agy.set(agy);
    }

    pub fn live(&self, rows: &[ForeignRow], now_ms: i64) -> ForeignLive {
        let mut live = ForeignLive::default();
        let Some(agy) = self.agy.get() else {
            return live;
        };
        let one = rows.len() == 1;
        for row in rows {
            let Some((Tool::Agy, id)) = foreign::parse(&row.id) else {
                continue;
            };
            let recent = (now_ms as f64) - row.since_ms < LIST_WITHIN_MS;
            if !one && !recent {
                if row.working {
                    live.agy_working.insert(id);
                }
                continue;
            }
            let read = agy.phone_read(&id, row.working);
            if read.working {
                live.agy_working.insert(id);
            }
            live.conversations.insert(row.id.clone(), read.conversation);
        }
        live
    }

    pub fn agents(&self, now_ms: i64) -> Vec<Agent> {
        if let Some((at, rows)) = lock(&self.cached).as_ref() {
            if now_ms >= *at && now_ms - *at < CACHE_MS {
                return rows.clone();
            }
        }
        let rows = self.read(now_ms);
        *lock(&self.cached) = Some((now_ms, rows.clone()));
        rows
    }

    pub fn rows(&self, now_ms: i64) -> Vec<ForeignRow> {
        self.agents(now_ms).iter().map(foreign_row).collect()
    }

    pub fn values(&self, now_ms: i64) -> Vec<Value> {
        self.agents(now_ms)
            .iter()
            .filter_map(|agent| {
                let mut value = serde_json::to_value(agent).ok()?;
                value["tool"] = json!(tool_of(&agent.id));
                Some(value)
            })
            .collect()
    }

    fn read(&self, now_ms: i64) -> Vec<Agent> {
        let mut rows = self.codex(now_ms);
        rows.extend(self.antigravity(now_ms));
        let hidden = delegate_jobs::hidden(&delegate_jobs::read(&self.data_dir));
        rows.retain(|agent| !hidden.contains(&agent.id));
        rows
    }

    fn codex(&self, now_ms: i64) -> Vec<Agent> {
        codex_list::rollouts(&self.codex_sessions, self.within_days, now_ms, CODEX_SCAN)
            .iter()
            .filter(|path| !is_child(path))
            .take(CODEX_ROWS)
            .filter_map(|path| codex_list::session(path))
            .collect()
    }

    fn antigravity(&self, now_ms: i64) -> Vec<Agent> {
        summaries::rows_cached(
            &self.agy_root,
            &self.agy_copy_dir,
            now_ms as f64 / 1000.0,
            AGY_REUSE,
        )
        .iter()
        .map(agy_agent)
        .collect()
    }
}

pub fn tool_of(row_id: &str) -> &'static str {
    if row_id.starts_with("codex:") {
        CODEX
    } else if row_id.starts_with("agy:") || row_id.starts_with("antigravity:") {
        ANTIGRAVITY
    } else {
        CLAUDE
    }
}

fn foreign_row(agent: &Agent) -> ForeignRow {
    ForeignRow {
        id: agent.id.clone(),
        name: agent.name.clone(),
        project: agent.project.clone(),
        cwd: agent.cwd.clone(),
        since_ms: agent.since_ms as f64,
        working: agent.group == AgentGroup::Working,
        transcript_path: agent.transcript_path.clone(),
    }
}

fn agy_agent(row: &Row) -> Agent {
    Agent {
        id: row.row_id(),
        name: if row.title.trim().is_empty() {
            "Antigravity Session".to_owned()
        } else {
            row.title.clone()
        },
        project: Path::new(&row.folder)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        cwd: row.folder.clone(),
        group: if row.running {
            AgentGroup::Working
        } else {
            AgentGroup::Completed
        },
        label: if row.running {
            "Working"
        } else {
            "Antigravity"
        }
        .to_owned(),
        detail: None,
        since_ms: (row.modified * 1000.0) as i64,
        target: AgentTarget::Resume {
            session_id: row.id.clone(),
            cwd: row.folder.clone(),
        },
        suggested_reply: None,
        transcript_path: None,
        host: "Antigravity".to_owned(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
    }
}

fn is_child(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::new();
    if file.take(HEAD_BYTES).read_to_end(&mut head).is_err() {
        return false;
    }
    let line = head.split(|&b| b == b'\n').next().unwrap_or_default();
    let Ok(first) = serde_json::from_slice::<Value>(line) else {
        return false;
    };
    let payload = &first["payload"];
    let named = |key: &str| payload[key].as_str().is_some_and(|s| !s.is_empty());
    named("parent_thread_id") || payload["thread_source"].as_str() == Some("subagent")
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
