use super::event::{plain_id, Event};
use super::rules::{Action, Rule, Would};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FILE: &str = "events.log";
pub const TAKEN: &str = "events.log.take";
pub const MAX_SPOOL: u64 = 1 << 20;
pub const MAX_READ: u64 = 4 << 20;
pub const TOOLS: [&str; 4] = ["Bash", "Read", "Agent", "other"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Logged {
    pub ts: f64,
    pub session: String,
    pub agent: String,
    pub tool: String,
    pub rule: String,
    pub action: String,
}

pub fn logged(now: f64, event: &Event, would: &Would) -> Logged {
    Logged {
        ts: now,
        session: event.session.clone(),
        agent: event.agent.clone().unwrap_or_default(),
        tool: event.logged_tool().to_string(),
        rule: would.rule.name().to_string(),
        action: would.action.name().to_string(),
    }
}

fn valid(row: &Logged) -> bool {
    row.ts.is_finite()
        && plain_id(&row.session)
        && (row.agent.is_empty() || plain_id(&row.agent))
        && TOOLS.contains(&row.tool.as_str())
        && Rule::parse(&row.rule).is_some()
        && Action::parse(&row.action).is_some()
}

#[cfg(unix)]
fn open_append(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_append(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

pub fn append(dir: &Path, now: f64, event: &Event, woulds: &[Would]) -> bool {
    if woulds.is_empty() {
        return true;
    }
    let mut text = String::new();
    for would in woulds {
        if let Ok(line) = serde_json::to_string(&logged(now, event, would)) {
            text.push_str(&line);
            text.push('\n');
        }
    }
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let path = dir.join(FILE);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= MAX_SPOOL) {
        return false;
    }
    open_append(&path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .is_ok()
}

#[derive(Debug)]
pub struct Taken {
    pub rows: Vec<Logged>,
    pub file: PathBuf,
}

impl Taken {
    pub fn done(self) {
        let _ = std::fs::remove_file(self.file);
    }
}

fn read_rows(path: &Path) -> Vec<Logged> {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    if file.take(MAX_READ).read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|l| serde_json::from_str::<Logged>(l).ok())
        .filter(valid)
        .collect()
}

pub fn take(dir: &Path) -> Option<Taken> {
    let log = dir.join(FILE);
    let taken = dir.join(TAKEN);
    if !taken.exists() {
        if !log.exists() {
            return None;
        }
        std::fs::rename(&log, &taken).ok()?;
    }
    Some(Taken {
        rows: read_rows(&taken),
        file: taken,
    })
}
