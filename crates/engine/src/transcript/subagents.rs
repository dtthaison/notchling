use super::glance::Peeked;
use super::model::{Conversation, Timestamp};
use super::util::{file_times, modified_secs};
use crate::contract::Tool;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

pub const QUIET_AFTER: f64 = 600.0;
pub const ACTIVE_WINDOW: f64 = 90.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Completed,
    Failed,
    Stopped,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Via {
    #[default]
    Native,
    Delegated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Origin {
    pub tool: Tool,
    pub via: Via,
}

impl Origin {
    pub const fn native(tool: Tool) -> Origin {
        Origin {
            tool,
            via: Via::Native,
        }
    }

    pub const fn delegated(tool: Tool) -> Origin {
        Origin {
            tool,
            via: Via::Delegated,
        }
    }
}

impl Default for Origin {
    fn default() -> Origin {
        Origin::native(Tool::Claude)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Subagent {
    pub id: String,
    pub path: String,
    pub description: String,
    pub agent_type: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub background: bool,
    pub tool_use_id: Option<String>,
    pub started: Timestamp,
    pub updated: Timestamp,
    pub status: Status,
    pub origin: Origin,
    pub session: String,
    pub parent: String,
    pub depth: u8,
}

impl Subagent {
    pub fn is_running(&self) -> bool {
        self.status == Status::Running
    }

    pub fn own_id(&self) -> &str {
        self.id
            .split_once(':')
            .map_or(self.id.as_str(), |(_, rest)| rest)
    }
}

pub fn folder(transcript_path: &str) -> String {
    let base = Path::new(transcript_path).with_extension("");
    base.join("subagents").to_string_lossy().into_owned()
}

fn meta_cache() -> &'static Mutex<HashMap<String, Value>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn read_meta(path: &str) -> Value {
    let mut cache = meta_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = cache.get(path) {
        return hit.clone();
    }
    let parsed = std::fs::read(path)
        .ok()
        .and_then(|d| serde_json::from_slice::<Value>(&d).ok())
        .filter(Value::is_object);
    match parsed {
        Some(obj) => {
            cache.insert(path.to_string(), obj.clone());
            obj
        }
        None => Value::Null,
    }
}

fn opt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(String::from)
}

pub fn list(
    transcript_path: &str,
    finished: &HashMap<String, String>,
    now: Timestamp,
) -> Vec<Subagent> {
    let dir = folder(transcript_path);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let session = Path::new(transcript_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("agent-") && name.ends_with(".jsonl")) {
            continue;
        }
        let id = name["agent-".len()..name.len() - ".jsonl".len()].to_string();
        let file = Path::new(&dir).join(&name).to_string_lossy().into_owned();
        let (updated, started) = file_times(&file).unwrap_or((0.0, 0.0));
        let meta = read_meta(
            &Path::new(&dir)
                .join(format!("agent-{id}.meta.json"))
                .to_string_lossy(),
        );
        let mut agent = Subagent {
            id: format!("claude:{id}"),
            path: file.clone(),
            description: opt_str(&meta, "description").unwrap_or_else(|| "Subagent".into()),
            agent_type: opt_str(&meta, "agentType").unwrap_or_else(|| "general-purpose".into()),
            model: opt_str(&meta, "model"),
            effort: opt_str(&meta, "effort"),
            background: opt_str(&meta, "requestShape").as_deref() == Some("background"),
            tool_use_id: opt_str(&meta, "toolUseId"),
            started,
            updated,
            status: Status::Quiet,
            origin: Origin::native(Tool::Claude),
            session: session.clone(),
            parent: session.clone(),
            depth: 1,
        };
        agent.status = status_of(&agent, finished, now);
        if agent.status != Status::Completed && agent.status != Status::Failed && handed_back(&file)
        {
            agent.status = Status::Completed;
        }
        out.push(agent);
    }
    out.sort_by(|a, b| a.started.total_cmp(&b.started));
    out
}

pub fn status_of(agent: &Subagent, finished: &HashMap<String, String>, now: Timestamp) -> Status {
    let word = finished.get(agent.own_id()).or_else(|| {
        if agent.background {
            None
        } else {
            agent.tool_use_id.as_ref().and_then(|t| finished.get(t))
        }
    });
    match word.map(String::as_str) {
        Some("completed") => Status::Completed,
        Some("failed" | "error") => Status::Failed,
        Some("killed" | "stopped" | "cancelled") => Status::Stopped,
        _ if now - agent.updated < QUIET_AFTER => Status::Running,
        _ => Status::Quiet,
    }
}

pub fn handed_back(path: &str) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let Ok(size) = file.seek(SeekFrom::End(0)) else {
        return false;
    };
    if file
        .seek(SeekFrom::Start(size.saturating_sub(8192)))
        .is_err()
    {
        return false;
    }
    let mut tail = Vec::new();
    if file.read_to_end(&mut tail).is_err() {
        return false;
    }
    let needle = b"Report delivered to your caller";
    tail.windows(needle.len()).any(|w| w == needle)
}

pub fn active_count(transcript_path: &str, now: Timestamp) -> usize {
    let dir = folder(transcript_path);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl"))
        .filter_map(|e| modified_secs(&e.path().to_string_lossy()))
        .filter(|modified| now - modified < ACTIVE_WINDOW)
        .count()
}

fn tag(name: &str, text: &str) -> Option<String> {
    let open = format!("<{name}>");
    let a = text.find(&open)? + open.len();
    let b = a + text[a..].find(&format!("</{name}>"))?;
    Some(text[a..b].trim().to_string())
}

pub fn notification_statuses(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("<task-notification>") {
        rest = &rest[open + "<task-notification>".len()..];
        let end = rest.find("</task-notification>").unwrap_or(rest.len());
        let block = &rest[..end];
        if let (Some(id), Some(status)) = (tag("task-id", block), tag("status", block)) {
            out.push((id, status));
        }
        rest = &rest[end..];
    }
    out
}

pub fn read(path: &str) -> Conversation {
    super::conversation::read_subagent(path)
}

pub fn read_shared(path: &str) -> std::sync::Arc<Conversation> {
    super::conversation::read_subagent_shared(path)
}

pub fn peek(origin: Origin, path: &str, with_recent: bool) -> Option<Peeked> {
    match origin.tool {
        Tool::Claude => super::conversation::peek_subagent(path, with_recent),
        Tool::Codex => super::codex::peek(path, with_recent),
        Tool::Antigravity => None,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsoleMeta {
    agent_type: Option<String>,
    description: Option<String>,
    tool_use_id: Option<String>,
    spawn_depth: Option<i64>,
    model: Option<String>,
    finished: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleSubagent {
    pub id: String,
    pub agent_type: String,
    pub description: String,
    pub model: String,
    pub depth: i64,
    pub parent_tool_use_id: Option<String>,
    pub finished: bool,
    pub last_activity: Timestamp,
    pub transcript_path: String,
    pub running: bool,
}

pub fn console_list(
    session_id: &str,
    transcript_path: Option<&str>,
    now: Timestamp,
    running_window: f64,
) -> Vec<ConsoleSubagent> {
    let Some(transcript) = transcript_path else {
        return Vec::new();
    };
    let Some(parent) = Path::new(transcript).parent() else {
        return Vec::new();
    };
    let dir = parent.join(session_id).join("subagents");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut agents = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("agent-") && name.ends_with(".meta.json")) {
            continue;
        }
        let meta_path = entry.path();
        let Some(meta) = std::fs::read(&meta_path)
            .ok()
            .and_then(|d| serde_json::from_slice::<ConsoleMeta>(&d).ok())
        else {
            continue;
        };
        let id = name["agent-".len()..name.len() - ".meta.json".len()].to_string();
        let jsonl = dir.join(format!("agent-{id}.jsonl"));
        let last_activity = modified_secs(&jsonl.to_string_lossy())
            .or_else(|| modified_secs(&meta_path.to_string_lossy()))
            .unwrap_or(0.0);
        let finished = meta.finished.unwrap_or(false);
        agents.push(ConsoleSubagent {
            id,
            agent_type: meta.agent_type.unwrap_or_else(|| "agent".into()),
            description: meta.description.unwrap_or_default(),
            model: meta.model.unwrap_or_default(),
            depth: meta.spawn_depth.unwrap_or(1),
            parent_tool_use_id: meta.tool_use_id,
            finished,
            last_activity,
            transcript_path: jsonl.to_string_lossy().into_owned(),
            running: !finished && now - last_activity < running_window,
        });
    }
    agents.sort_by(|a, b| {
        b.running
            .cmp(&a.running)
            .then(b.last_activity.total_cmp(&a.last_activity))
    });
    agents
}

#[cfg(test)]
#[path = "subagents_shape_tests.rs"]
mod shape_tests;
