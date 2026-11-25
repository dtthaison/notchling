use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::shells_stop::Proc;
use crate::sessions::jobs::{pid_of, read_json};
use crate::sessions::task_output::plain_id;
use crate::sessions::{config_dirs, Roots};

pub fn tmp_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for base in [
        PathBuf::from("/tmp"),
        PathBuf::from("/private/tmp"),
        std::env::temp_dir(),
    ] {
        let real = std::fs::canonicalize(&base).ok();
        for root in std::iter::once(base).chain(real) {
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
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

fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn lstart_epoch(text: &str) -> Option<i64> {
    crate::sessions::fresh::lstart_ms(text).map(|ms| ms.div_euclid(1000))
}

#[cfg(target_os = "linux")]
fn ticks_of(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

#[cfg(not(target_os = "linux"))]
fn ticks_of(_: i32) -> Option<u64> {
    None
}

fn same_count(count: u64, proc: &Proc) -> bool {
    if ticks_of(proc.pid) == Some(count) {
        return true;
    }
    let Some(began) = lstart_epoch(&proc.started).and_then(|at| u64::try_from(at).ok()) else {
        return false;
    };
    let secs = if count >= 100_000_000_000 {
        count / 1000
    } else {
        count
    };
    secs.abs_diff(began) <= 2
}

fn same_start(recorded: &Value, proc: &Proc) -> bool {
    let text = match recorded {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        _ => return true,
    };
    if text.is_empty() {
        return true;
    }
    if let Ok(count) = text.parse::<u64>() {
        return same_count(count, proc);
    }
    words(&text) == words(&proc.started)
        || lstart_epoch(&text).is_some_and(|at| lstart_epoch(&proc.started) == Some(at))
}

fn claude_code(proc: &Proc) -> bool {
    matches!(proc.name(), "claude" | "node")
        || proc.claude_titled()
        || proc.command.contains("/claude/")
        || proc.command.contains("/claude-code/")
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
            let live = i32::try_from(pid)
                .ok()
                .and_then(|pid| procs.iter().find(|p| p.pid == pid))
                .is_some_and(|p| {
                    !p.zombie
                        && claude_code(p)
                        && row
                            .get("procStart")
                            .is_none_or(|recorded| same_start(recorded, p))
                });
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
