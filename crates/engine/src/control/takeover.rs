use crate::sessions::{Agent, AgentTarget, Roots};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TakeoverState {
    Available,
    Stoppable { busy: bool },
    Unavailable(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TakeoverError {
    Alive,
    NoTranscript,
    Sdk,
    AlreadyControllable,
    NoWorkingFolder,
}

pub const ALIVE_REASON: &str =
    "Đóng session ở nơi đang mở (Claude desktop / VS Code) rồi tiếp quản";

impl std::fmt::Display for TakeoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TakeoverError::Alive => ALIVE_REASON,
            TakeoverError::NoTranscript => "Không có transcript tiếp tục được trong ~/.claude/projects, nên không tiếp quản được.",
            TakeoverError::Sdk => "Session chạy bằng claude -p, không tiếp quản được.",
            TakeoverError::AlreadyControllable => "Console đã điều khiển được session này.",
            TakeoverError::NoWorkingFolder => "Thư mục làm việc của session không còn.",
        })
    }
}

impl std::error::Error for TakeoverError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    ViewOnly,
    ConsolePty,
    Job,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TakeoverSession {
    pub id: String,
    pub pid: Option<u32>,
    pub cwd: String,
    pub sdk: bool,
    pub control: Control,
    pub running: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TakeoverPlan {
    pub session_id: String,
    pub cwd: String,
    pub transcript: PathBuf,
    pub arguments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TakeoverSpawn {
    pub executable: String,
    pub arguments: Vec<String>,
    pub cwd: String,
    pub environment: BTreeMap<String, String>,
}

pub struct TakeoverProbes<'a> {
    pub is_alive: &'a dyn Fn(u32) -> bool,
    pub dir_exists: &'a dyn Fn(&str) -> bool,
}

fn real_dir_exists(path: &str) -> bool {
    Path::new(path).is_dir()
}

impl TakeoverProbes<'static> {
    pub fn with_alive(is_alive: &'static dyn Fn(u32) -> bool) -> Self {
        TakeoverProbes {
            is_alive,
            dir_exists: &real_dir_exists,
        }
    }
}

fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

pub fn resumable_transcript(session_id: &str, projects_dir: &Path) -> Option<PathBuf> {
    if !is_uuid(session_id) {
        return None;
    }
    let mut folders: Vec<PathBuf> = std::fs::read_dir(projects_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    folders.sort();
    folders
        .into_iter()
        .map(|f| f.join(format!("{session_id}.jsonl")))
        .find(|candidate| candidate.is_file())
}

fn has_live_session_file(
    session_id: &str,
    sessions_dir: &Path,
    is_alive: &dyn Fn(u32) -> bool,
) -> bool {
    let Ok(entries) = std::fs::read_dir(sessions_dir) else {
        return false;
    };
    entries.flatten().map(|e| e.path()).any(|path| {
        path.extension().is_some_and(|e| e == "json")
            && std::fs::read(&path)
                .ok()
                .and_then(|d| serde_json::from_slice::<serde_json::Value>(&d).ok())
                .is_some_and(|v| {
                    v.get("sessionId").and_then(|s| s.as_str()) == Some(session_id)
                        && v.get("pid")
                            .and_then(|p| p.as_u64())
                            .and_then(|p| u32::try_from(p).ok())
                            .is_some_and(is_alive)
                })
    })
}

pub fn evaluate(
    session: &TakeoverSession,
    roots: &Roots,
    assume_stopped: bool,
    probes: &TakeoverProbes,
) -> Result<TakeoverPlan, TakeoverError> {
    if session.control != Control::ViewOnly {
        return Err(TakeoverError::AlreadyControllable);
    }
    if session.sdk {
        return Err(TakeoverError::Sdk);
    }
    if !assume_stopped && session.running {
        return Err(TakeoverError::Alive);
    }
    if session.pid.is_some_and(probes.is_alive) {
        return Err(TakeoverError::Alive);
    }
    if has_live_session_file(
        &session.id,
        &roots.main_dir.join("sessions"),
        probes.is_alive,
    ) {
        return Err(TakeoverError::Alive);
    }
    if session.cwd.is_empty() || !session.cwd.starts_with('/') || !(probes.dir_exists)(&session.cwd)
    {
        return Err(TakeoverError::NoWorkingFolder);
    }
    let transcript = resumable_transcript(&session.id, &roots.main_dir.join("projects"))
        .ok_or(TakeoverError::NoTranscript)?;
    Ok(TakeoverPlan {
        session_id: session.id.clone(),
        cwd: session.cwd.clone(),
        transcript,
        arguments: vec!["--resume".to_string(), session.id.clone()],
    })
}

pub fn state(
    session: &TakeoverSession,
    roots: &Roots,
    probes: &TakeoverProbes,
    original_stoppable: &dyn Fn(&TakeoverSession) -> bool,
) -> Option<TakeoverState> {
    if session.control != Control::ViewOnly {
        return None;
    }
    Some(match evaluate(session, roots, false, probes) {
        Ok(_) => TakeoverState::Available,
        Err(TakeoverError::Alive) => {
            let never_alive = |_: u32| false;
            let after_stop = TakeoverProbes {
                is_alive: &never_alive,
                dir_exists: probes.dir_exists,
            };
            let rest = evaluate(session, roots, true, &after_stop);
            if rest.is_ok() && original_stoppable(session) {
                TakeoverState::Stoppable {
                    busy: session.running,
                }
            } else {
                TakeoverState::Unavailable(TakeoverError::Alive.to_string())
            }
        }
        Err(error) => TakeoverState::Unavailable(error.to_string()),
    })
}

pub fn scrubbed(environment: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut result: BTreeMap<String, String> = environment
        .iter()
        .filter(|(k, _)| !k.starts_with("CLAUDE_CODE_") && k.as_str() != "CLAUDECODE")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    result.insert("TERM".to_string(), "xterm-256color".to_string());
    result.insert("COLORTERM".to_string(), "truecolor".to_string());
    result
}

pub fn spawn(
    plan: &TakeoverPlan,
    executable: &str,
    environment: &BTreeMap<String, String>,
) -> TakeoverSpawn {
    TakeoverSpawn {
        executable: executable.to_string(),
        arguments: plan.arguments.clone(),
        cwd: plan.cwd.clone(),
        environment: scrubbed(environment),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    SameSession,
    Replaced { old: String, new: String },
}

pub fn resolution(original: &str, resumed: &str) -> Resolution {
    if original == resumed {
        Resolution::SameSession
    } else {
        Resolution::Replaced {
            old: original.to_string(),
            new: resumed.to_string(),
        }
    }
}

pub fn session_id_for_root_pid(
    root_pid: u32,
    sessions_dir: &Path,
    parent_of: &dyn Fn(u32) -> Option<u32>,
) -> Option<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(sessions_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    files.into_iter().find_map(|path| {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        let pid = u32::try_from(v.get("pid")?.as_u64()?).ok()?;
        let id = v.get("sessionId")?.as_str()?.to_string();
        descends(pid, root_pid, parent_of).then_some(id)
    })
}

fn descends(pid: u32, root: u32, parent_of: &dyn Fn(u32) -> Option<u32>) -> bool {
    let mut current = pid;
    for _ in 0..8 {
        if current == root {
            return true;
        }
        match parent_of(current) {
            Some(parent) => current = parent,
            None => return false,
        }
    }
    false
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub title: String,
    pub message: String,
    pub confirm: String,
    pub destructive: bool,
}

pub fn offers_takeover(agent: &Agent, owned_by_host: bool) -> bool {
    match &agent.target {
        AgentTarget::Job(_) => false,
        AgentTarget::Process(_) => !owned_by_host && agent.host != "claude -p",
        AgentTarget::Resume { .. } => true,
    }
}

pub fn prompt_for(state: Option<&TakeoverState>, name: &str) -> Option<Prompt> {
    let prompt = |title: String, message: &str, confirm: &str, destructive: bool| Prompt {
        title,
        message: message.to_string(),
        confirm: confirm.to_string(),
        destructive,
    };
    Some(match state? {
        TakeoverState::Available => prompt(
            format!("Take Over \u{201c}{name}\u{201d}?"),
            "Notchling runs claude --resume for it in the session host, in the same folder. The original has ended, so nothing writes twice. It then takes replies from Chat and the phone.",
            "Take Over",
            false,
        ),
        TakeoverState::Stoppable { .. } => prompt(
            format!("Take Over \u{201c}{name}\u{201d}?"),
            "Notchling stops the original (Claude desktop, VS Code or a terminal) and resumes the session in the session host. It then takes replies from Chat and the phone.",
            "Stop Original and Take Over",
            true,
        ),
        TakeoverState::Unavailable(why) => prompt(
            format!("Can\u{2019}t Take Over \u{201c}{name}\u{201d}"),
            why,
            "OK",
            false,
        ),
    })
}

pub fn busy_prompt() -> Prompt {
    Prompt {
        title: "Session Is Running".to_string(),
        message: "Its current turn stops midway.".to_string(),
        confirm: "Stop and Take Over".to_string(),
        destructive: true,
    }
}

#[cfg(test)]
#[path = "takeover_tests.rs"]
mod tests;
