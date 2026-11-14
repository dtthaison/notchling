use std::collections::HashSet;
use std::path::{Path, PathBuf};

use notchling_engine::delegate::record::{self, Delegation, End, State};
use notchling_engine::tools::agy::elsewhere;
use notchling_engine::tools::codex::list::find_rollout;
use notchling_engine::transcript::locate::predecessor;
use notchling_engine::transcript::model::Timestamp;
use notchling_engine::transcript::subagents::{Origin, Status, Subagent};
use notchling_locale as t;

const MAX_BACK: usize = 4;

pub fn folder(data_dir: &Path) -> PathBuf {
    data_dir.join(record::FOLDER)
}

pub fn read(data_dir: &Path) -> Vec<Delegation> {
    record::read_all(&folder(data_dir))
}

pub fn hidden(records: &[Delegation]) -> HashSet<String> {
    record::hidden_rows(records).into_iter().collect()
}

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

pub struct Places<'a> {
    pub codex_sessions: &'a Path,
    pub antigravity: &'a Path,
}

pub fn subagents(
    records: &[Delegation],
    parents: &[String],
    places: &Places<'_>,
    now: Timestamp,
    alive: &dyn Fn(u32) -> bool,
) -> Vec<Subagent> {
    records
        .iter()
        .filter(|r| parents.contains(&r.parent_session))
        .map(|r| subagent(r, places, now, alive))
        .collect()
}

pub fn subagent(
    record: &Delegation,
    places: &Places<'_>,
    now: Timestamp,
    alive: &dyn Fn(u32) -> bool,
) -> Subagent {
    let (origin, title) = match record.tool.as_str() {
        "agy" => (Origin::Antigravity, t::chat::delegated_antigravity()),
        _ => (Origin::Codex, t::chat::delegated_codex()),
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
