use super::agent::{Agent, AgentGroup, AgentTarget};
use super::iso::parse_iso_ms;
use super::lanes::{config_dirs, Roots};
use crate::transcript::locate::job_transcript;
use crate::transcript::model::Timestamp;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rank {
    pub live: bool,
    pub updated_ms: i64,
}

pub fn rank(job: &Value) -> Rank {
    Rank {
        live: str_field(job, "state") != Some("stopped"),
        updated_ms: str_field(job, "updatedAt")
            .and_then(parse_iso_ms)
            .unwrap_or(i64::MIN),
    }
}

pub fn newest_first(copies: &[(Option<String>, Value)]) -> Vec<Option<String>> {
    let mut ranked: Vec<(Option<String>, Rank)> = copies
        .iter()
        .map(|(dir, job)| (dir.clone(), rank(job)))
        .collect();
    ranked.sort_by_key(|a| std::cmp::Reverse(a.1));
    ranked.into_iter().map(|(dir, _)| dir).collect()
}

pub fn job_dirs(roots: &Roots, id: &str) -> Vec<Option<String>> {
    let copies: Vec<(Option<String>, Value)> = config_dirs(roots)
        .into_iter()
        .filter_map(|c| {
            let state = read_json(&c.dir.join("jobs").join(id).join("state.json"))?;
            Some((c.lane.map(|l| l.path), state))
        })
        .collect();
    newest_first(&copies)
}

pub fn config_dir_for_job(roots: &Roots, id: &str) -> Option<String> {
    job_dirs(roots, id).into_iter().next().flatten()
}

pub const FIRST_PROMPT: &str = "send a prompt to start";

pub fn agent_from_job(
    job: &Value,
    short_id: &str,
    live: Option<&Value>,
    modified: &dyn Fn(&str) -> Option<Timestamp>,
) -> Agent {
    let state = str_field(job, "state").unwrap_or("");
    let tempo = str_field(job, "tempo");
    let status = live.and_then(|l| str_field(l, "status"));

    let unprompted = str_field(job, "needs") == Some(FIRST_PROMPT)
        && matches!(state, "working" | "running" | "")
        && status != Some("waiting");

    let (group, label) = if matches!(status, Some("busy" | "shell")) {
        (AgentGroup::Working, "Working".to_string())
    } else if unprompted {
        (AgentGroup::Completed, "Idle".to_string())
    } else if state == "blocked" || tempo == Some("blocked") || status == Some("waiting") {
        (AgentGroup::NeedsInput, "Needs input".to_string())
    } else if status == Some("idle") {
        (AgentGroup::Completed, "Idle".to_string())
    } else if state == "working" || tempo == Some("active") {
        (AgentGroup::Working, "Working".to_string())
    } else if state == "running" || state.is_empty() {
        (AgentGroup::Completed, "Idle".to_string())
    } else {
        (AgentGroup::Completed, capitalized(state))
    };

    let detail = if group == AgentGroup::NeedsInput {
        str_field(job, "needs").or_else(|| str_field(job, "detail"))
    } else {
        str_field(job, "detail")
    }
    .map(String::from);
    let cwd = str_field(job, "cwd").unwrap_or("").to_string();
    let mut since_ms = str_field(job, "updatedAt")
        .and_then(parse_iso_ms)
        .unwrap_or(0);
    if let Some(ms) = live
        .and_then(|l| l.get("statusUpdatedAt"))
        .and_then(Value::as_f64)
    {
        since_ms = since_ms.max(ms as i64);
    }
    let session_id = str_field(job, "sessionId");

    Agent {
        id: session_id.unwrap_or(short_id).to_string(),
        name: str_field(job, "name").unwrap_or(short_id).to_string(),
        project: last_component(&cwd),
        cwd,
        group,
        label,
        detail,
        since_ms,
        target: AgentTarget::Job(short_id.to_string()),
        suggested_reply: str_field(job, "suggestedReply").map(String::from),
        transcript_path: job_transcript(
            str_field(job, "linkScanPath"),
            session_id,
            str_field(job, "worktreePath"),
            modified,
        ),
        host: "Background".to_string(),
        headless: false,
        config_dir: None,
        command: (str_field(job, "template") == Some("exec"))
            .then(|| str_field(job, "intent").unwrap_or("").to_string()),
        resume_session_id: str_field(job, "resumeSessionId").map(String::from),
    }
}

pub fn agent_from_session(s: &Value, host_app: &dyn Fn(u32) -> Option<String>) -> Option<Agent> {
    let id = str_field(s, "sessionId")?;
    let pid = pid_of(s)?;
    let status = str_field(s, "status").unwrap_or("idle");
    let cwd = str_field(s, "cwd").unwrap_or("").to_string();
    let (group, label) = match status {
        "waiting" => (AgentGroup::NeedsInput, "Waiting for you"),
        "busy" | "shell" => (AgentGroup::Working, "Working"),
        _ => (AgentGroup::Completed, "Idle"),
    };
    let ms = ["statusUpdatedAt", "updatedAt", "startedAt"]
        .iter()
        .find_map(|k| s.get(*k).and_then(Value::as_f64))
        .unwrap_or(0.0);
    let headless = str_field(s, "entrypoint") == Some("sdk-cli");
    Some(Agent {
        id: id.to_string(),
        name: str_field(s, "name").map_or_else(|| last_component(&cwd), String::from),
        project: last_component(&cwd),
        cwd,
        group,
        label: label.to_string(),
        detail: None,
        since_ms: ms as i64,
        target: AgentTarget::Process(pid),
        suggested_reply: None,
        transcript_path: None,
        host: if headless {
            "claude -p".to_string()
        } else {
            host_app(pid).unwrap_or_else(|| "Terminal".to_string())
        },
        headless,
        config_dir: None,
        command: None,
        resume_session_id: None,
    })
}

pub fn read_jobs(base: &Path) -> BTreeMap<String, Value> {
    let mut jobs = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(base.join("jobs")) else {
        return jobs;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if let Some(state) = read_json(&entry.path().join("state.json")) {
            jobs.insert(name, state);
        }
    }
    jobs
}

pub fn read_sessions(base: &Path, alive: &dyn Fn(u32) -> bool) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(base.join("sessions")) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|p| read_json(p))
        .filter(|s| pid_of(s).is_some_and(alive))
        .collect()
}

pub fn launch_flags(job: &Value, name: &str) -> Vec<String> {
    let Some(flags) = job.get("respawnFlags").and_then(Value::as_array) else {
        return vec!["--name".to_string(), name.to_string()];
    };
    let mut out = Vec::new();
    let mut skip_next = false;
    for flag in flags.iter().filter_map(Value::as_str) {
        if skip_next {
            skip_next = false;
        } else if flag == "--resume" || flag == "-r" {
            skip_next = true;
        } else if flag != "--bg" && flag != "--background" {
            out.push(flag.to_string());
        }
    }
    out
}

pub fn resume_arguments(conversation: &str, existing: bool, flags: &[String]) -> Vec<String> {
    let mut args = if existing { Vec::new() } else { flags.to_vec() };
    args.extend(["--bg", "--resume", conversation].map(String::from));
    args
}

fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn pid_of(s: &Value) -> Option<u32> {
    let n = s.get("pid")?;
    n.as_u64()
        .or_else(|| n.as_f64().map(|f| f as u64))
        .and_then(|p| u32::try_from(p).ok())
}

pub(crate) fn read_json(path: &Path) -> Option<Value> {
    let data = std::fs::read(path).ok()?;
    serde_json::from_slice(&data).ok().filter(Value::is_object)
}

pub(crate) fn last_component(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.is_empty() {
            String::new()
        } else {
            "/".to_string()
        };
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
}

fn capitalized(text: &str) -> String {
    let mut out = String::new();
    let mut start = true;
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(if start {
                c.to_uppercase().collect::<Vec<_>>()
            } else {
                c.to_lowercase().collect()
            });
            start = false;
        } else {
            out.push(c);
            start = true;
        }
    }
    out
}
