use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::shells_stop::Proc;
use crate::sessions::jobs::{pid_of, read_json};
use crate::sessions::task_output::plain_id;
use crate::sessions::{config_dirs, Roots};

pub fn tmp_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ["/tmp", "/private/tmp"].iter().map(PathBuf::from).collect();
    let own = std::env::temp_dir();
    if !roots.contains(&own) {
        roots.push(own);
    }
    roots
}

#[cfg(unix)]
pub fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(not(unix))]
pub fn current_uid() -> u32 {
    0
}

fn real_dir(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_dir())
}

pub fn shell_output(
    path: &str,
    session: &str,
    id: &str,
    roots: &[PathBuf],
    uid: u32,
) -> Option<PathBuf> {
    if !plain_id(id) || !plain_id(session) {
        return None;
    }
    let path = Path::new(path);
    let user = format!("claude-{uid}");
    let file = format!("{id}.output");
    roots.iter().find_map(|root| {
        let rest = path.strip_prefix(root).ok()?;
        let parts: Vec<&str> = rest
            .components()
            .map(|c| match c {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .collect::<Option<_>>()?;
        let [first, project, owner, tasks, name] = parts[..] else {
            return None;
        };
        if first != user || project.is_empty() || owner != session || tasks != "tasks" {
            return None;
        }
        if name != file {
            return None;
        }
        let mut walked = root.clone();
        for part in [first, project, owner, tasks] {
            walked.push(part);
            if !real_dir(&walked) {
                return None;
            }
        }
        path.symlink_metadata()
            .ok()
            .filter(|meta| meta.file_type().is_file())
            .map(|_| path.to_path_buf())
    })
}

fn same_start(recorded: &str, started: &str) -> bool {
    let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    recorded.trim().parse::<u64>().is_ok() || words(recorded) == words(started)
}

pub fn session_owner(roots: &Roots, session: &str, procs: &[Proc]) -> Option<u32> {
    if !plain_id(session) {
        return None;
    }
    let mut found: Vec<u32> = Vec::new();
    for lane in config_dirs(roots) {
        let Ok(entries) = std::fs::read_dir(lane.dir.join("sessions")) else {
            continue;
        };
        for path in entries.flatten().map(|e| e.path()) {
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let Some(row) = read_json(&path) else {
                continue;
            };
            if row.get("sessionId").and_then(Value::as_str) != Some(session) {
                continue;
            }
            let Some(pid) = pid_of(&row) else { continue };
            let recorded = row.get("procStart").and_then(Value::as_str).unwrap_or("");
            let live = i32::try_from(pid)
                .ok()
                .and_then(|pid| procs.iter().find(|p| p.pid == pid))
                .is_some_and(|p| !p.zombie && same_start(recorded, &p.started));
            if live && !found.contains(&pid) {
                found.push(pid);
            }
        }
    }
    match found[..] {
        [only] => Some(only),
        _ => None,
    }
}

#[cfg(test)]
#[path = "shells_guard_tests.rs"]
mod tests;
