//! Jobs a session handed to Codex or Antigravity CLI (`notchling delegate`), shown as that
//! session's subagents. The command leaves a record per job (`engine::delegate::record`); here the
//! records become what Chat shows: a [`Subagent`] in the Details Panel of the session that handed
//! the job over, and no Sidebar row or notch row for the tool's own session.
//!
//! The parent is matched by the id the record holds, Claude Code's session id, which is the file
//! stem of the session's transcript. A `/clear` starts a new transcript for the same session row, so
//! the transcripts it ended count too (`locate::predecessor`): the jobs stay with the row you see.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use notchling_engine::delegate::record::{self, Delegation, End, State};
use notchling_engine::tools::agy::elsewhere;
use notchling_engine::tools::codex::list::find_rollout;
use notchling_engine::transcript::locate::predecessor;
use notchling_engine::transcript::model::Timestamp;
use notchling_engine::transcript::subagents::{Origin, Status, Subagent};

/// How many clears in a row are followed back to find a session's jobs.
const MAX_BACK: usize = 4;

/// Where the records are, given Notchling's data folder.
pub fn folder(data_dir: &Path) -> PathBuf {
    data_dir.join(record::FOLDER)
}

/// The records, oldest first.
pub fn read(data_dir: &Path) -> Vec<Delegation> {
    record::read_all(&folder(data_dir))
}

/// The Sidebar and notch row ids (`codex:<id>`, `agy:<id>`) that are jobs of another session.
pub fn hidden(records: &[Delegation]) -> HashSet<String> {
    record::hidden_rows(records).into_iter().collect()
}

/// The ids a record may name for the session whose transcript is at `path`: its own, and those of
/// the transcripts its clears ended.
pub fn parent_ids(path: &str, name: &str) -> Vec<String> {
    let stem = |p: &str| {
        Path::new(p)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
    };
    let mut ids: Vec<String> = stem(path).into_iter().collect();
    let mut current = path.to_string();
    for _ in 0..MAX_BACK {
        let Some(before) = predecessor(&current, name) else {
            break;
        };
        ids.extend(stem(&before));
        current = before;
    }
    ids
}

/// Where the tools keep what a job's transcript is read from.
pub struct Places<'a> {
    /// Codex's `sessions` folder.
    pub codex_sessions: &'a Path,
    /// `~/.gemini/antigravity-cli`.
    pub antigravity: &'a Path,
}

/// The records `parents` handed over, as subagents (oldest first, as the list is). `alive` says
/// whether a process exists, so a job whose command was killed doesn't run for ever.
pub fn subagents(
    records: &[Delegation],
    parents: &[String],
    places: &Places<'_>,
    now: Timestamp,
    alive: &dyn Fn(u32) -> bool,
) -> Vec<Subagent> {
    records
        .iter()
        .filter(|r| parents.iter().any(|p| *p == r.parent_session))
        .map(|r| subagent(r, places, now, alive))
        .collect()
}

/// One job as a subagent. Its transcript is the tool's own: Codex's rollout, or Antigravity's log.
pub fn subagent(
    record: &Delegation,
    places: &Places<'_>,
    now: Timestamp,
    alive: &dyn Fn(u32) -> bool,
) -> Subagent {
    let (origin, title) = match record.tool.as_str() {
        "agy" => (Origin::Antigravity, "Antigravity CLI"),
        _ => (Origin::Codex, "Codex"),
    };
    let path = record
        .session
        .as_deref()
        .and_then(|session| match origin {
            Origin::Antigravity => {
                let log = elsewhere::log_path(places.antigravity, session);
                log.is_file().then_some(log)
            }
            _ => find_rollout(places.codex_sessions, session),
        })
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let status = match record.state(now, alive) {
        State::Running => Status::Running,
        State::Ended(End::Ok) => Status::Completed,
        State::Ended(End::Failed) => Status::Failed,
        State::Ended(End::Stopped) => Status::Stopped,
    };
    Subagent {
        id: record.id(),
        path,
        description: record.description.clone(),
        agent_type: title.to_string(),
        model: record.model.clone(),
        effort: record.effort.clone(),
        background: false,
        tool_use_id: None,
        started: record.started,
        updated: record.ended.unwrap_or(now),
        status,
        origin,
    }
}

#[cfg(test)]
#[path = "delegated_tests.rs"]
mod tests;
