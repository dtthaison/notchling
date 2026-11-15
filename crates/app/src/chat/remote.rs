use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notchling_engine::cluster::blob::safe_name;
use notchling_engine::cluster::Qualified;
use notchling_engine::sessions::Agent;
use notchling_engine::transcript::model::{ChatItem, Conversation, ItemKind};
use notchling_locale as t;
use serde_json::Value;

use super::data::Session;
use crate::notch::requests::HookRequest;

pub const FOLLOW_EVERY_MS: i64 = 60_000;
pub const LOG_TAIL_BYTES: u64 = 64 * 1024;
pub const SHELL_PREFIX: &str = "shell:";
const SHELL_ITEM: &str = "shell-log";

pub fn is_remote(agent: &Agent) -> bool {
    Qualified::parse(&agent.id).is_some()
}

pub fn follow_due(selected: Option<&Session>, last_ms: i64, now_ms: i64) -> bool {
    selected.is_some_and(|s| is_remote(&s.agent))
        && now_ms.saturating_sub(last_ms) >= FOLLOW_EVERY_MS
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Local,
    Mirror(PathBuf),
    Pending,
}

fn inside_mirror(path: &Path) -> bool {
    let parts: Vec<_> = path.components().map(|c| c.as_os_str()).collect();
    path.is_absolute()
        && parts
            .windows(2)
            .any(|w| w[0] == "cluster" && w[1] == "mirror")
        && parts.iter().all(|p| *p != ".." && *p != ".")
}

pub fn source(agent: &Agent) -> Source {
    if !is_remote(agent) {
        return Source::Local;
    }
    match agent.transcript_path.as_deref().map(Path::new) {
        Some(path) if inside_mirror(path) => Source::Mirror(path.to_path_buf()),
        _ => Source::Pending,
    }
}

pub fn waiting_for_mirror(session: &Session) -> bool {
    match source(&session.agent) {
        Source::Local => false,
        Source::Pending => true,
        Source::Mirror(_) => session.transcript.is_none(),
    }
}

fn bare_id(agent: &Agent) -> &str {
    agent
        .id
        .split_once('/')
        .filter(|_| is_remote(agent))
        .map_or(agent.id.as_str(), |(_, id)| id)
}

pub fn is_shell(agent: &Agent) -> bool {
    agent.command.is_some() && bare_id(agent).starts_with(SHELL_PREFIX)
}

pub fn shell_log(agent: &Agent) -> Option<PathBuf> {
    if !is_shell(agent) {
        return None;
    }
    match source(agent) {
        Source::Local => agent
            .transcript_path
            .as_deref()
            .map(PathBuf::from)
            .filter(|p| p.is_absolute() && p.is_file()),
        Source::Mirror(path) => path.is_file().then_some(path),
        Source::Pending => None,
    }
}

pub fn log_tail(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    let start = size.saturating_sub(LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(LOG_TAIL_BYTES).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start == 0 {
        return Ok(text);
    }
    Ok(match text.split_once('\n') {
        Some((_, rest)) => rest.to_owned(),
        None => text,
    })
}

fn fence_for(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat(longest.max(2) + 1)
}

pub fn shell_conversation(log: &str) -> Conversation {
    let body = log.trim_end_matches('\n');
    if body.is_empty() {
        return Conversation::default();
    }
    let fence = fence_for(body);
    let text = format!("{fence}\n{body}\n{fence}");
    Conversation {
        items: vec![ChatItem {
            id: SHELL_ITEM.to_owned(),
            kind: ItemKind::Assistant { text },
            date: None,
        }],
        ..Conversation::default()
    }
}

pub fn shell_conversation_shared(path: &Path) -> Arc<Conversation> {
    Arc::new(
        log_tail(path)
            .map(|log| shell_conversation(&log))
            .unwrap_or_default(),
    )
}

pub fn job_of_worktree(cwd: &str) -> Option<String> {
    if !cwd.starts_with('/') || cwd.contains('\0') {
        return None;
    }
    let parts: Vec<&str> = cwd[1..].split('/').collect();
    let trailing = parts.last() == Some(&"");
    let parts = if trailing {
        &parts[..parts.len() - 1]
    } else {
        &parts[..]
    };
    if parts
        .iter()
        .any(|p| p.is_empty() || *p == "." || *p == "..")
    {
        return None;
    }
    let at = parts
        .windows(4)
        .position(|w| w[0] == "cluster" && w[1] == "work" && w[2] == "jobs")?;
    let job = parts[at + 3];
    safe_name(job).then(|| job.to_owned())
}

pub fn bring_back_job(agent: &Agent) -> Option<String> {
    if !is_remote(agent) {
        return None;
    }
    job_of_worktree(&agent.cwd)
}

pub fn brought_back(reply: &Value) -> Option<String> {
    reply
        .get("branch")
        .and_then(Value::as_str)
        .filter(|b| !b.is_empty())
        .map(str::to_owned)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BringBack {
    Busy,
    Done(String),
    Failed(String),
}

impl BringBack {
    pub fn text(&self) -> String {
        match self {
            BringBack::Busy => t::cluster::bring_back_busy().to_owned(),
            BringBack::Done(branch) => t::cluster::brought_back(branch),
            BringBack::Failed(error) => t::cluster::bring_back_failed(error),
        }
    }

    pub fn failed(&self) -> bool {
        matches!(self, BringBack::Failed(_))
    }
}

pub fn bring_back_outcome(reply: Result<Value, String>) -> BringBack {
    match reply {
        Ok(value) => match brought_back(&value) {
            Some(branch) => BringBack::Done(branch),
            None => BringBack::Failed(t::cluster::bring_back_no_branch().to_owned()),
        },
        Err(error) => BringBack::Failed(error),
    }
}

pub fn held_prompt(agent: &Agent, list: &[Value]) -> Option<HookRequest> {
    let mirror = agent.transcript_path.as_deref();
    list.iter()
        .filter(|p| {
            let named = p.get("sessionId").and_then(Value::as_str) == Some(agent.id.as_str());
            let path = p.get("transcriptPath").and_then(Value::as_str);
            named || (mirror.is_some() && path == mirror)
        })
        .filter_map(HookRequest::from_prompt)
        .filter(|r| r.agy.is_none() && Qualified::parse(&r.id).is_some())
        .map(|mut r| {
            r.can_always_allow = false;
            r
        })
        .min_by(|a, b| a.created.total_cmp(&b.created))
}
