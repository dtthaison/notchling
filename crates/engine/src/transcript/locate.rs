//! Finding a session's transcript file: Claude Code's project folder naming, a background job that
//! moved into a worktree, and the new file a `/clear` starts.

use super::model::Timestamp;
use super::util::{file_times, modified_secs};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// Claude Code's folder name for a working directory: every character other than an ASCII letter or
/// digit becomes "-" ("/Users/me/.claude" becomes "-Users-me--claude").
pub fn project_folder(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// `<projects dir>/<project folder>/<session id>.jsonl`, found directly from the working directory
/// (also under the `/private` prefix macOS's `/tmp`, `/var` and `/etc` links carry), else by looking
/// in every project folder.
pub fn find_transcript(projects_dir: &str, session_id: &str, cwd: &str) -> Option<String> {
    let file = format!("{session_id}.jsonl");
    let mut cwds = vec![cwd.to_string()];
    if let Some(rest) = cwd.strip_prefix("/private") {
        cwds.push(rest.to_string());
    } else if ["/tmp", "/var", "/etc"]
        .iter()
        .any(|p| cwd == *p || cwd.starts_with(&format!("{p}/")))
    {
        cwds.push(format!("/private{cwd}"));
    }
    for c in cwds {
        let guess = Path::new(projects_dir).join(project_folder(&c)).join(&file);
        if guess.is_file() {
            return Some(guess.to_string_lossy().into_owned());
        }
    }
    let mut dirs: Vec<_> = std::fs::read_dir(projects_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .map(|d| d.join(&file))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

/// A background job's transcript. A session that enters a git worktree goes on in that worktree's
/// project folder, while `state.json` keeps naming the file in the folder it started in, which Claude
/// Code has moved away. So the worktree's folder (in the same projects folder, which is the lane's for
/// a lane job) is tried too, and the newer of the two files wins. With neither on disk it returns
/// `link_scan_path` as before. `modified` reads a file's modification time.
pub fn job_transcript(
    link_scan_path: Option<&str>,
    session_id: Option<&str>,
    worktree_path: Option<&str>,
    modified: impl Fn(&str) -> Option<Timestamp>,
) -> Option<String> {
    let link = link_scan_path?;
    let mut candidates = vec![link.to_string()];
    if let (Some(session), Some(worktree)) = (
        session_id.filter(|s| !s.is_empty()),
        worktree_path.filter(|w| !w.is_empty()),
    ) {
        let projects = Path::new(link)
            .parent()
            .and_then(Path::parent)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        candidates.push(format!(
            "{projects}/{}/{session}.jsonl",
            project_folder(worktree)
        ));
    }
    let mut best: Option<(String, Timestamp)> = None;
    for path in candidates {
        let Some(date) = modified(&path) else {
            continue;
        };
        if best.as_ref().is_none_or(|(_, d)| *d < date) {
            best = Some((path, date));
        }
    }
    Some(best.map_or_else(|| link.to_string(), |b| b.0))
}

/// `job_transcript` reading real modification times.
pub fn job_transcript_on_disk(
    link_scan_path: Option<&str>,
    session_id: Option<&str>,
    worktree_path: Option<&str>,
) -> Option<String> {
    job_transcript(link_scan_path, session_id, worktree_path, modified_secs)
}

type SuccessorCache = HashMap<String, (SystemTime, Option<String>)>;

fn successor_cache() -> &'static Mutex<SuccessorCache> {
    static CACHE: OnceLock<Mutex<SuccessorCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The transcript a background job moved to after /clear. Clearing starts a new file, but the job's
/// state keeps naming the old one until its next link scan; the new file starts with the job's name
/// and a SessionStart:clear hook. Follows several clears in a row.
///
/// The search lists the whole project folder and reads every file's dates, and it runs for every agent
/// on every refresh, so its answer is kept until the folder's modification date changes. Appending to
/// a transcript leaves that date alone; creating a file (a new transcript after /clear) moves it.
pub fn successor(path: &str, name: &str) -> Option<String> {
    let dir = Path::new(path).parent()?.to_string_lossy().into_owned();
    let Some(dir_modified) = std::fs::metadata(&dir).ok().and_then(|m| m.modified().ok()) else {
        return search_successor(path, name);
    };
    let key = format!("{path}\n{name}");
    if let Some((stamp, result)) = successor_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
    {
        if *stamp == dir_modified {
            return result.clone();
        }
    }
    let result = search_successor(path, name);
    let mut cache = successor_cache().lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() > 500 {
        cache.clear();
    }
    cache.insert(key, (dir_modified, result.clone()));
    result
}

fn head_contains_clear(path: &Path, marker: &str) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let mut head = Vec::new();
    if file.take(16_384).read_to_end(&mut head).is_err() {
        return false;
    }
    let head = String::from_utf8_lossy(&head);
    head.contains(marker) && head.contains("SessionStart:clear")
}

fn search_successor(path: &str, name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let quoted = serde_json::to_string(name).ok()?;
    let marker = format!("\"agentName\":{quoted}");
    let dir = Path::new(path).parent()?.to_path_buf();

    let mut current = path.to_string();
    let mut found = None;
    for _ in 0..4 {
        let Some(since) = modified_secs(&current).map(|m| m - 10.0) else {
            break;
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            break;
        };
        // Oldest first, so a chain of clears is followed in order.
        let mut candidates: Vec<(Timestamp, String)> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| *p != current && p != path)
            .filter_map(|p| file_times(&p).map(|(_, created)| (created, p)))
            .filter(|(created, _)| *created > since)
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let next = candidates
            .into_iter()
            .map(|c| c.1)
            .find(|p| head_contains_clear(Path::new(p), &marker));
        let Some(next) = next else { break };
        found = Some(next.clone());
        current = next;
    }
    found
}
