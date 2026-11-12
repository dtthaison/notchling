//! A delegation's record: when an agent hands a job to Codex or Antigravity (`notchling delegate`),
//! the command leaves a small JSON file in `<data>/delegations/` saying which session handed it
//! over and what the job was. Chat reads those files to show the job as a subagent of that session,
//! in the Details Panel, and to keep its session out of the Sidebar and the notch.
//!
//! The command makes one only when Claude Code's session id is in its environment
//! (`CLAUDE_CODE_SESSION_ID`, which Claude Code sets in every Bash call); a person at a terminal
//! delegates as before and the tool's session stays a row of its own. A record that can't be written
//! never fails the job: it is a convenience for the list.
//!
//! The file is `<tool>-<tool session id>.json`, written again at each step (the id is known only
//! once the tool says it, so until then it is `<tool>-pending-<pid>.json`), each time atomically
//! with mode 0600. `ended` and `outcome` are set when the command exits. A record with no `ended`
//! whose process is gone (killed, a closed laptop) counts as ended and stopped.

use super::options::Options;
use super::outcome::Outcome;
use notchling_platform::files;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The folder under Notchling's data folder.
pub const FOLDER: &str = "delegations";
/// The environment variable Claude Code gives every command it runs: the session's id.
pub const PARENT_VARIABLE: &str = "CLAUDE_CODE_SESSION_ID";
/// The longest a description is, in characters.
pub const DESCRIPTION_CHARS: usize = 80;
/// A record that never ended and is older than this counts as ended even if its pid is alive again
/// (the pid was reused after a restart).
pub const STALE_AFTER_SECS: f64 = 24.0 * 3600.0;
/// Records older than this are removed when a new delegation starts.
pub const KEEP_SECS: f64 = 30.0 * 24.0 * 3600.0;

/// How the job ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum End {
    Ok,
    Failed,
    Stopped,
}

/// Where a job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Ended(End),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delegation {
    pub version: u32,
    /// Claude Code's id of the session that handed the job over (its transcript's file stem).
    pub parent_session: String,
    pub parent_cwd: String,
    /// `codex` or `agy`.
    pub tool: String,
    /// The tool's own session id, once known.
    pub session: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The first line of the prompt.
    pub description: String,
    /// Seconds since the epoch.
    pub started: f64,
    pub ended: Option<f64>,
    pub outcome: Option<End>,
    /// The `notchling delegate` process.
    pub pid: u32,
}

impl Delegation {
    pub const VERSION: u32 = 1;

    /// A job that has just started.
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

    /// The file's name: the session's id when it is known, else the process's.
    pub fn file_name(&self) -> String {
        match &self.session {
            Some(session) if plain_id(session) => format!("{}-{session}.json", self.tool),
            _ => format!("{}-pending-{}.json", self.tool, self.pid),
        }
    }

    /// A name for the job that doesn't change when its session gets an id: Chat's subagent list
    /// and an open subagent keep following it through that.
    pub fn id(&self) -> String {
        format!(
            "{}-{}-{}",
            self.tool,
            self.pid,
            (self.started * 1000.0) as i64
        )
    }

    /// The Sidebar row of the tool's session (`codex:<id>`, `agy:<id>`), once there is one.
    pub fn row_id(&self) -> Option<String> {
        let session = self.session.as_deref().filter(|s| plain_id(s))?;
        match self.tool.as_str() {
            "codex" => Some(format!("codex:{session}")),
            "agy" => Some(format!("agy:{session}")),
            _ => None,
        }
    }

    /// Running while its process lives and it has not said it ended; `alive` answers for a pid.
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

/// An id that is safe in a file name and a row id: letters, digits and `-`.
fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The first line of the prompt that says something, cut to [`DESCRIPTION_CHARS`] with an
/// ellipsis; "Delegated job" for a prompt of only blank lines.
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

/// Writes the record's file in `dir` (made 0700 if missing).
pub fn write(dir: &Path, record: &Delegation) -> std::io::Result<()> {
    files::ensure_private_dir(dir).map_err(std::io::Error::other)?;
    let json = serde_json::to_vec_pretty(record).map_err(std::io::Error::other)?;
    files::write_private_no_follow(&dir.join(record.file_name()), &json)
        .map_err(std::io::Error::other)
}

/// Every record in `dir`, oldest first. A file that isn't one (or is from a newer version) is left
/// out.
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

/// The Sidebar row ids that have a record: those sessions are subagents, not rows.
pub fn hidden_rows(records: &[Delegation]) -> Vec<String> {
    records.iter().filter_map(Delegation::row_id).collect()
}

/// Removes the records that started more than [`KEEP_SECS`] ago and are not running.
pub fn prune(dir: &Path, now: f64, alive: &dyn Fn(u32) -> bool) {
    for record in read_all(dir) {
        if now - record.started > KEEP_SECS && record.state(now, alive) != State::Running {
            let _ = std::fs::remove_file(dir.join(record.file_name()));
        }
    }
}

/// Whether a process with this id exists. Not being allowed to signal it still means it exists.
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

/// The record of the run in progress, written as it goes. Every method ignores a write that fails.
pub struct Recorder {
    dir: PathBuf,
    record: Mutex<Delegation>,
}

impl Recorder {
    /// Starts the record: prunes old ones, writes the first (the job is listed as running at once).
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

    /// The tool named its session; its model and effort too when they are known now.
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

    /// The command is about to exit with this outcome. `stopped`: a signal asked it to stop.
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

/// How an outcome counts: stopped when a signal asked for it, failed on any error (a timeout
/// too), else done.
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
