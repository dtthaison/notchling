//! Listing the files worth indexing: Claude Code transcripts under every config folder's
//! `projects`, and Codex rollouts under `sessions/YYYY/MM/DD`. Only metadata is read here.

use super::{ClaudeRoot, SessionMeta, SessionTool, SESSION_WINDOW_SECS};
use crate::pages::clock::days_from_civil;
use crate::search::codex_id;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// A file that may need reading.
#[derive(Clone, Debug)]
pub(super) struct Candidate {
    pub meta: SessionMeta,
    pub tool: SessionTool,
}

/// Seconds since 1970 of a file's last change, and its size.
fn stat(path: &Path) -> Option<(f64, i64)> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let modified = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs_f64();
    Some((modified, i64::try_from(meta.len()).unwrap_or(i64::MAX)))
}

/// Claude Code transcripts: `<projects>/<folder>/<session id>.jsonl`, changed within the window.
/// A lane's `projects` is usually a link to `~/.claude/projects`, so a file is taken once however
/// it is reached; the same session in two lanes (moved between accounts) keeps the newer copy.
pub(super) fn claude(roots: &[ClaudeRoot], now: f64) -> Vec<Candidate> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out: HashMap<String, Candidate> = HashMap::new();
    for root in roots {
        let Ok(folders) = fs::read_dir(&root.projects) else {
            continue;
        };
        for folder in folders.flatten() {
            let Ok(files) = fs::read_dir(folder.path()) else {
                continue;
            };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().into_owned();
                let Some(id) = name.strip_suffix(".jsonl") else {
                    continue;
                };
                let path = file.path();
                let Some((modified, size)) = stat(&path) else {
                    continue;
                };
                if now - modified >= SESSION_WINDOW_SECS {
                    continue;
                }
                let real = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if !seen.insert(real) {
                    continue;
                }
                let key = format!("claude:{id}");
                if out.get(&key).is_some_and(|c| c.meta.modified >= modified) {
                    continue;
                }
                out.insert(
                    key.clone(),
                    Candidate {
                        meta: SessionMeta {
                            key,
                            tool_key: SessionTool::Claude.key().into(),
                            session_id: id.to_string(),
                            title: String::new(),
                            folder: String::new(),
                            path: path.to_string_lossy().into_owned(),
                            config_dir: root.config_dir.clone(),
                            modified,
                            size,
                        },
                        tool: SessionTool::Claude,
                    },
                );
            }
        }
    }
    out.into_values().collect()
}

/// A number from a folder name made only of digits.
fn number(name: &std::ffi::OsStr) -> Option<i64> {
    let s = name.to_str()?;
    (!s.is_empty() && s.len() <= 4 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

fn sub_dirs(dir: &Path) -> Vec<(i64, PathBuf)> {
    fs::read_dir(dir)
        .map(|read| {
            read.flatten()
                .filter_map(|e| Some((number(&e.file_name())?, e.path())))
                .collect()
        })
        .unwrap_or_default()
}

/// Codex rollouts: `<sessions>/YYYY/MM/DD/rollout-*.jsonl`, changed within the window. Like the
/// Swift app, only day folders from the window's span are entered (the folder is the day the
/// rollout started; one day more is allowed for the time zone it was named in), so a session
/// that started earlier is not found however recently it was written to.
pub(super) fn codex(sessions: &Path, now: f64) -> Vec<Candidate> {
    let oldest_day = ((now - SESSION_WINDOW_SECS) / 86_400.0).floor() as i64 - 1;
    let mut out = Vec::new();
    for (year, year_dir) in sub_dirs(sessions) {
        for (month, month_dir) in sub_dirs(&year_dir) {
            if !(1..=12).contains(&month) {
                continue;
            }
            for (day, day_dir) in sub_dirs(&month_dir) {
                if !(1..=31).contains(&day) || days_from_civil(year, month, day) < oldest_day {
                    continue;
                }
                let Ok(files) = fs::read_dir(&day_dir) else {
                    continue;
                };
                for file in files.flatten() {
                    let name = file.file_name().to_string_lossy().into_owned();
                    if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                        continue;
                    }
                    let path = file.path();
                    let Some((modified, size)) = stat(&path) else {
                        continue;
                    };
                    if now - modified >= SESSION_WINDOW_SECS {
                        continue;
                    }
                    let id = codex_id(&name);
                    out.push(Candidate {
                        meta: SessionMeta {
                            key: format!("codex:{id}"),
                            tool_key: SessionTool::Codex.key().into(),
                            session_id: id,
                            title: String::new(),
                            folder: String::new(),
                            path: path.to_string_lossy().into_owned(),
                            config_dir: None,
                            modified,
                            size,
                        },
                        tool: SessionTool::Codex,
                    });
                }
            }
        }
    }
    out
}
