use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::contract::subagents::{Roots, SessionRef};
use crate::contract::Tool;
use crate::delegate::record::{self, Delegation, End, State};
use crate::tools::agy::elsewhere;
use crate::tools::codex::list::find_rollout;
use crate::transcript::locate::predecessor;
use crate::transcript::model::Timestamp;
use crate::transcript::subagents::{Origin, Status, Subagent};

pub const MAX_BACK: usize = 4;

pub fn folder(data_dir: &Path) -> PathBuf {
    data_dir.join(record::FOLDER)
}

pub fn read_folder(dir: &Path) -> Vec<Delegation> {
    record::read_all(dir)
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

pub fn parents_of(parent: &SessionRef) -> Vec<String> {
    match (&parent.transcript, parent.tool) {
        (Some(path), Tool::Claude) if parent.id == parent.session => {
            parent_ids(&path.to_string_lossy(), &parent.name)
        }
        _ => vec![parent.own_id().to_owned()],
    }
}

pub fn subagents(
    records: &[Delegation],
    parent: &SessionRef,
    roots: &Roots,
    now: Timestamp,
    alive: &dyn Fn(u32) -> bool,
) -> Vec<Subagent> {
    if records.is_empty() {
        return Vec::new();
    }
    select(records, &parents_of(parent), parent, roots, now, alive)
}

pub fn select(
    records: &[Delegation],
    parents: &[String],
    parent: &SessionRef,
    roots: &Roots,
    now: Timestamp,
    alive: &dyn Fn(u32) -> bool,
) -> Vec<Subagent> {
    records
        .iter()
        .filter(|r| parents.contains(&r.parent_session))
        .map(|r| subagent(r, parent, roots, now, alive))
        .collect()
}

pub fn subagent(
    record: &Delegation,
    parent: &SessionRef,
    roots: &Roots,
    now: Timestamp,
    alive: &dyn Fn(u32) -> bool,
) -> Subagent {
    let (tool, title) = match record.tool.as_str() {
        "agy" => (Tool::Antigravity, "Antigravity CLI"),
        _ => (Tool::Codex, "Codex"),
    };
    let path = record
        .session
        .as_deref()
        .and_then(|session| match tool {
            Tool::Antigravity => {
                let log = elsewhere::log_path(&roots.antigravity, session);
                log.is_file().then_some(log)
            }
            _ => find_rollout(&roots.codex_sessions, session),
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
        id: format!("job:{}", record.id()),
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
        origin: Origin::delegated(tool),
        session: parent.session.clone(),
        parent: parent.id.clone(),
        depth: 1,
    }
}

#[cfg(test)]
#[path = "delegate_jobs_tests.rs"]
mod tests;
