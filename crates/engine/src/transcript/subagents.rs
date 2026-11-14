use super::glance::Peeked;
use super::model::{Conversation, Timestamp};
use super::tail::RAW_BYTES;
use super::util::{file_times, modified_secs};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

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
pub enum Origin {
    #[default]
    Claude,
    Codex,
    Antigravity,
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
}

impl Subagent {
    pub fn is_running(&self) -> bool {
        self.status == Status::Running
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
    list_chain(&[transcript_path], finished, now)
}

fn apply_meta(agent: &mut Subagent, meta: &Value) {
    agent.description = opt_str(meta, "description").unwrap_or_else(|| "Subagent".into());
    agent.agent_type = opt_str(meta, "agentType").unwrap_or_else(|| "general-purpose".into());
    agent.model = opt_str(meta, "model");
    agent.effort = opt_str(meta, "effort");
    agent.background = opt_str(meta, "requestShape").as_deref() == Some("background");
    agent.tool_use_id = opt_str(meta, "toolUseId");
}

pub fn list_chain<S: AsRef<str>>(
    newest_first: &[S],
    finished: &HashMap<String, String>,
    now: Timestamp,
) -> Vec<Subagent> {
    let mut out: Vec<Subagent> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut with_meta: Vec<bool> = Vec::new();
    for transcript in newest_first {
        let dir = folder(transcript.as_ref());
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("agent-") && name.ends_with(".jsonl")) {
                continue;
            }
            let id = name["agent-".len()..name.len() - ".jsonl".len()].to_string();
            let meta = read_meta(
                &Path::new(&dir)
                    .join(format!("agent-{id}.meta.json"))
                    .to_string_lossy(),
            );
            if let Some(&known) = at.get(&id) {
                if !with_meta[known] && !meta.is_null() {
                    apply_meta(&mut out[known], &meta);
                    with_meta[known] = true;
                }
                continue;
            }
            let file = Path::new(&dir).join(&name).to_string_lossy().into_owned();
            let (updated, started) = file_times(&file).unwrap_or((0.0, 0.0));
            let mut agent = Subagent {
                id: id.clone(),
                path: file,
                description: String::new(),
                agent_type: String::new(),
                model: None,
                effort: None,
                background: false,
                tool_use_id: None,
                started,
                updated,
                status: Status::Quiet,
                origin: Origin::Claude,
            };
            apply_meta(&mut agent, &meta);
            at.insert(id, out.len());
            with_meta.push(!meta.is_null());
            out.push(agent);
        }
    }
    for agent in &mut out {
        agent.status = status_of(agent, finished, now);
        if agent.status != Status::Completed
            && agent.status != Status::Failed
            && handed_back_stamped(&agent.path)
        {
            agent.status = Status::Completed;
        }
    }
    out.sort_by(|a, b| a.started.total_cmp(&b.started));
    out
}

pub fn status_of(agent: &Subagent, finished: &HashMap<String, String>, now: Timestamp) -> Status {
    let word = finished.get(&agent.id).or_else(|| {
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

type HandedBack = HashMap<String, (u64, Option<SystemTime>, bool)>;

fn handed_back_cache() -> &'static Mutex<HandedBack> {
    static CACHE: OnceLock<Mutex<HandedBack>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn handed_back_stamped(path: &str) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let stamp = (meta.len(), meta.modified().ok());
    if let Some((len, modified, answer)) = handed_back_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(path)
    {
        if (*len, *modified) == stamp {
            return *answer;
        }
    }
    let answer = handed_back(path);
    let mut cache = handed_back_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if cache.len() > 5000 {
        cache.clear();
    }
    cache.insert(path.to_string(), (stamp.0, stamp.1, answer));
    answer
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
    RAW_BYTES.fetch_add(tail.len() as u64, Ordering::Relaxed);
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

pub fn result_end<'a>(
    tool_use_id: Option<&'a str>,
    is_error: bool,
    result: Option<&'a Value>,
) -> Option<([&'a str; 2], &'static str)> {
    let tool = tool_use_id?;
    let result = result?;
    let agent = result.get("agentId").and_then(Value::as_str)?;
    if result.get("status").and_then(Value::as_str) == Some("async_launched") {
        return None;
    }
    Some(([tool, agent], if is_error { "failed" } else { "completed" }))
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
    match origin {
        Origin::Claude => super::conversation::peek_subagent(path, with_recent),
        Origin::Codex => super::codex::peek(path, with_recent),
        Origin::Antigravity => None,
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
