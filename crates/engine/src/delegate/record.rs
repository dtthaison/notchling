use super::options::Options;
use super::outcome::Outcome;
use notchling_platform::files;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const FOLDER: &str = "delegations";
pub const PARENT_VARIABLE: &str = "CLAUDE_CODE_SESSION_ID";
pub const DESCRIPTION_CHARS: usize = 80;
pub const STALE_AFTER_SECS: f64 = 24.0 * 3600.0;
pub const KEEP_SECS: f64 = 30.0 * 24.0 * 3600.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum End {
    Ok,
    Failed,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Ended(End),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delegation {
    pub version: u32,
    pub parent_session: String,
    pub parent_cwd: String,
    pub tool: String,
    pub session: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub description: String,
    pub started: f64,
    pub ended: Option<f64>,
    pub outcome: Option<End>,
    pub pid: u32,
}

impl Delegation {
    pub const VERSION: u32 = 1;

    pub fn new(
        parent_session: &str,
        parent_cwd: &str,
        options: &Options,
        now: f64,
        pid: u32,
    ) -> Delegation {
        Delegation {
            version: Delegation::VERSION,
            parent_session: parent_session.to_string(),
            parent_cwd: parent_cwd.to_string(),
            tool: options.tool.raw().to_string(),
            session: options.session.clone().filter(|s| plain_id(s)),
            model: options.model.clone(),
            effort: options.effort.clone(),
            description: describe(&options.prompt),
            started: now,
            ended: None,
            outcome: None,
            pid,
        }
    }

    pub fn file_name(&self) -> String {
        match &self.session {
            Some(session) if plain_id(session) => format!("{}-{session}.json", self.tool),
            _ => format!("{}-pending-{}.json", self.tool, self.pid),
        }
    }

    pub fn id(&self) -> String {
        format!(
            "{}-{}-{}",
            self.tool,
            self.pid,
            (self.started * 1000.0) as i64
        )
    }

    pub fn row_id(&self) -> Option<String> {
        let session = self.session.as_deref().filter(|s| plain_id(s))?;
        match self.tool.as_str() {
            "codex" => Some(format!("codex:{session}")),
            "agy" => Some(format!("agy:{session}")),
            _ => None,
        }
    }

    pub fn state(&self, now: f64, alive: &dyn Fn(u32) -> bool) -> State {
        if self.ended.is_some() {
            return State::Ended(self.outcome.unwrap_or(End::Ok));
        }
        if now - self.started > STALE_AFTER_SECS || !alive(self.pid) {
            return State::Ended(End::Stopped);
        }
        State::Running
    }
}

fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub fn describe(prompt: &str) -> String {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if line.is_empty() {
        return "Delegated job".into();
    }
    if line.chars().count() <= DESCRIPTION_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(DESCRIPTION_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

pub fn write(dir: &Path, record: &Delegation) -> std::io::Result<()> {
    files::ensure_private_dir(dir).map_err(std::io::Error::other)?;
    let json = serde_json::to_vec_pretty(record).map_err(std::io::Error::other)?;
    files::write_private_no_follow(&dir.join(record.file_name()), &json)
        .map_err(std::io::Error::other)
}

pub fn read_all(dir: &Path) -> Vec<Delegation> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Delegation> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|data| serde_json::from_slice::<Delegation>(&data).ok())
        .filter(|d| d.version <= Delegation::VERSION)
        .collect();
    out.sort_by(|a, b| {
        a.started
            .total_cmp(&b.started)
            .then_with(|| a.file_name().cmp(&b.file_name()))
    });
    out
}

pub fn hidden_rows(records: &[Delegation]) -> Vec<String> {
    records.iter().filter_map(Delegation::row_id).collect()
}

pub fn prune(dir: &Path, now: f64, alive: &dyn Fn(u32) -> bool) {
    for record in read_all(dir) {
        if now - record.started > KEEP_SECS && record.state(now, alive) != State::Running {
            let _ = std::fs::remove_file(dir.join(record.file_name()));
        }
    }
}

#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 only checks that the process exists.
    let rc = unsafe { libc::kill(pid, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
pub fn pid_alive(_pid: u32) -> bool {
    true
}

pub struct Recorder {
    dir: PathBuf,
    record: Mutex<Delegation>,
}

impl Recorder {
    pub fn start(dir: &Path, record: Delegation, now: f64) -> Recorder {
        prune(dir, now, &pid_alive);
        let recorder = Recorder {
            dir: dir.to_path_buf(),
            record: Mutex::new(record),
        };
        recorder.save(|_| {});
        recorder
    }

    fn save(&self, change: impl FnOnce(&mut Delegation)) {
        let mut record = self.record.lock().unwrap_or_else(|e| e.into_inner());
        let before = record.file_name();
        change(&mut record);
        let _ = write(&self.dir, &record);
        let after = record.file_name();
        if before != after {
            let _ = std::fs::remove_file(self.dir.join(before));
        }
    }

    pub fn session_known(&self, session: &str, model: Option<&str>, effort: Option<&str>) {
        if !plain_id(session) {
            return;
        }
        self.save(|r| {
            r.session = Some(session.to_string());
            if let Some(m) = model {
                r.model = Some(m.to_string());
            }
            if let Some(e) = effort {
                r.effort = Some(e.to_string());
            }
        });
    }

    pub fn finish(&self, outcome: &Outcome, stopped: bool, now: f64) {
        let end = end_of(outcome, stopped);
        self.save(|r| {
            if let Some(session) = outcome.session.as_deref().filter(|s| plain_id(s)) {
                r.session = Some(session.to_string());
            }
            if outcome.model.is_some() {
                r.model = outcome.model.clone();
            }
            if outcome.effort.is_some() {
                r.effort = outcome.effort.clone();
            }
            r.ended = Some(now);
            r.outcome = Some(end);
        });
    }
}

pub fn end_of(outcome: &Outcome, stopped: bool) -> End {
    if stopped {
        End::Stopped
    } else if outcome.error.is_some() {
        End::Failed
    } else {
        End::Ok
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
