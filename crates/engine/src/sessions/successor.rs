//! A job's transcript: its worktree folder and the file `/clear` started.
//!
//! `state.json` names the transcript the job started in (`linkScanPath`). Two things move it on:
//! entering a git worktree (the session then writes in the worktree's project folder), and `/clear`
//! (a new file whose first lines carry the job's name and a `SessionStart:clear` hook).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Claude Code's folder name for a working directory: every character other than an ASCII letter or
/// digit becomes `-` (`/Users/me/.claude` becomes `-Users-me--claude`).
pub fn project_folder(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// A background job's transcript. A session that enters a git worktree goes on in that worktree's
/// project folder, while `state.json` keeps naming the file in the folder it started in, which Claude
/// Code has moved away. So the worktree's folder (in the same projects folder, which is the lane's for
/// a lane job) is tried too, and the newer of the two files wins. With neither on disk it returns
/// `link_scan_path`. `modified` gives a file's modification time in milliseconds, `None` if absent.
pub fn job_transcript(
    link_scan_path: Option<&str>,
    session_id: Option<&str>,
    worktree_path: Option<&str>,
    modified: &dyn Fn(&str) -> Option<i64>,
) -> Option<String> {
    let start = link_scan_path?;
    let mut candidates = vec![start.to_string()];
    if let (Some(id), Some(tree)) = (session_id, worktree_path) {
        if !id.is_empty() && !tree.is_empty() {
            let projects = parent(parent(start));
            candidates.push(format!("{projects}/{}/{id}.jsonl", project_folder(tree)));
        }
    }
    let mut best: Option<(String, i64)> = None;
    for path in candidates {
        if let Some(when) = modified(&path) {
            // `>=` keeps the later candidate on a tie, as `max` does in the Swift code.
            if best.as_ref().is_none_or(|(_, b)| when >= *b) {
                best = Some((path, when));
            }
        }
    }
    Some(best.map(|(p, _)| p).unwrap_or_else(|| start.to_string()))
}

/// A file's modification time in milliseconds since the epoch.
pub fn file_modified_ms(path: &str) -> Option<i64> {
    let when = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    Some(ms(when))
}

fn ms(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(e) => -(e.duration().as_millis() as i64),
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The transcript a background job moved to after `/clear`. Clearing starts a new file, but the job's
/// state keeps naming the old one until its next link scan; the new file starts with the job's name and
/// a `SessionStart:clear` hook. Follows several clears in a row. Lists the whole folder, so use
/// [`Successors`] when asking for many jobs often.
pub fn search_successor(path: &str, name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let quoted = serde_json::to_string(name).ok()?;
    let marker = format!("\"agentName\":{quoted}");
    let dir = parent(path);
    let mut current = path.to_string();
    let mut found: Option<String> = None;
    for _ in 0..4 {
        let since = modified_time(&current)? .checked_sub(Duration::from_secs(10))?;
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.ends_with(".jsonl"))
            .collect();
        names.sort();
        let next = names
            .into_iter()
            .map(|n| format!("{dir}/{n}"))
            .filter(|p| *p != current && p != path)
            .filter(|p| created_time(p).is_some_and(|c| c > since))
            .find(|p| started_by_clear(p, &marker));
        match next {
            Some(p) => {
                found = Some(p.clone());
                current = p;
            }
            None => break,
        }
    }
    found
}

fn modified_time(path: &str) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Creation time; where the file system has none, the modification time.
fn created_time(path: &str) -> Option<SystemTime> {
    let meta = std::fs::metadata(path).ok()?;
    meta.created().or_else(|_| meta.modified()).ok()
}

fn started_by_clear(path: &str, marker: &str) -> bool {
    let Ok(file) = std::fs::File::open(Path::new(path)) else { return false };
    let mut head = Vec::new();
    if file.take(16_384).read_to_end(&mut head).is_err() {
        return false;
    }
    let text = String::from_utf8_lossy(&head);
    text.contains(marker) && text.contains("SessionStart:clear")
}

/// [`search_successor`] with its answers kept until the transcript folder's modification date changes.
/// Appending to a transcript leaves that date alone; creating a file (the new transcript after
/// `/clear`) moves it.
#[derive(Default)]
pub struct Successors {
    cache: HashMap<String, (SystemTime, Option<String>)>,
}

impl Successors {
    pub fn find(&mut self, path: &str, name: &str) -> Option<String> {
        let Some(dir_modified) = modified_time(parent(path)) else {
            return search_successor(path, name);
        };
        let key = format!("{path}\n{name}");
        if let Some((when, result)) = self.cache.get(&key) {
            if *when == dir_modified {
                return result.clone();
            }
        }
        let result = search_successor(path, name);
        if self.cache.len() > 500 {
            self.cache.clear();
        }
        self.cache.insert(key, (dir_modified, result.clone()));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::TempDir;
    use super::*;
    use std::time::Duration;

    // JobTranscriptTests
    const START: &str = "/h/.claude/projects/-Users-me-app/abc.jsonl";
    const MOVED: &str = "/h/.claude/projects/-Users-me-app--claude-worktrees-music/abc.jsonl";
    const TREE: &str = "/Users/me/app/.claude/worktrees/music";

    #[test]
    fn follows_the_session_into_its_worktree() {
        let found = job_transcript(Some(START), Some("abc"), Some(TREE), &|p| (p == MOVED).then_some(1));
        assert_eq!(found.as_deref(), Some(MOVED));
    }

    #[test]
    fn the_newer_file_wins_when_both_exist() {
        let newer_moved = |p: &str| Some(if p == START { 100 } else { 200 });
        assert_eq!(job_transcript(Some(START), Some("abc"), Some(TREE), &newer_moved).as_deref(), Some(MOVED));
        let newer_start = |p: &str| Some(if p == START { 300 } else { 200 });
        assert_eq!(job_transcript(Some(START), Some("abc"), Some(TREE), &newer_start).as_deref(), Some(START));
    }

    #[test]
    fn without_a_worktree_it_is_the_recorded_path() {
        assert_eq!(job_transcript(Some(START), Some("abc"), None, &|_| None).as_deref(), Some(START));
        assert_eq!(job_transcript(None, Some("abc"), Some("/w"), &|_| Some(1)), None);
        assert_eq!(job_transcript(Some(START), Some(""), Some(TREE), &|_| Some(1)).as_deref(), Some(START));
    }

    // ProjectFolderTests: checked against real folder names in ~/.claude/projects.
    #[test]
    fn folder_names() {
        assert_eq!(project_folder("/Users/sondoan/go/src/pp/notchling"), "-Users-sondoan-go-src-pp-notchling");
        assert_eq!(project_folder("/Users/sondoan/.claude/jobs/45ee33db/tmp"), "-Users-sondoan--claude-jobs-45ee33db-tmp");
        assert_eq!(project_folder("/private/tmp/claude-501/x_y"), "-private-tmp-claude-501-x-y");
        assert_eq!(project_folder("/tmp/caf\u{e9}"), "-tmp-caf-");
    }

    // SuccessorTests
    fn cleared(agent: &str) -> String {
        format!("{{\"type\":\"system\",\"agentName\":\"{agent}\",\"content\":\"SessionStart:clear\"}}\n")
    }

    /// Folder dates have fine resolution, but give the file system a moment so they surely differ.
    fn tick() {
        std::thread::sleep(Duration::from_millis(60));
    }

    fn path(tmp: &TempDir, name: &str) -> String {
        tmp.path(name).display().to_string()
    }

    #[test]
    fn finds_the_transcript_started_by_clear() {
        let tmp = TempDir::new("succ1");
        tmp.write("old.jsonl", "{}\n");
        tick();
        tmp.write("new.jsonl", &cleared("worker"));
        let old = path(&tmp, "old.jsonl");
        assert_eq!(search_successor(&old, "worker"), Some(path(&tmp, "new.jsonl")));
        assert_eq!(search_successor(&old, "someone-else"), None);
        assert_eq!(search_successor(&old, ""), None);
    }

    #[test]
    fn follows_several_clears_in_a_row() {
        let tmp = TempDir::new("succ-chain");
        tmp.write("a.jsonl", "{}\n");
        tick();
        tmp.write("b.jsonl", &cleared("worker"));
        tick();
        tmp.write("c.jsonl", &cleared("worker"));
        assert_eq!(search_successor(&path(&tmp, "a.jsonl"), "worker"), Some(path(&tmp, "c.jsonl")));
    }

    #[test]
    fn a_new_file_is_seen_after_a_cached_miss() {
        let tmp = TempDir::new("succ2");
        tmp.write("old.jsonl", "{}\n");
        let old = path(&tmp, "old.jsonl");
        let mut cache = Successors::default();
        assert_eq!(cache.find(&old, "worker"), None);
        tick();
        tmp.write("new.jsonl", &cleared("worker"));
        assert_eq!(cache.find(&old, "worker"), Some(path(&tmp, "new.jsonl")), "creating a file must invalidate the cache");
    }

    #[test]
    fn appending_does_not_rescan() {
        use std::io::Write;
        let tmp = TempDir::new("succ3");
        tmp.write("old.jsonl", "{}\n");
        let old = path(&tmp, "old.jsonl");
        let before = std::fs::metadata(tmp.root()).and_then(|m| m.modified()).ok();
        tick();
        let mut file = std::fs::OpenOptions::new().append(true).open(&old).expect("open");
        file.write_all(b"{\"more\":1}\n").expect("append");
        drop(file);
        let after = std::fs::metadata(tmp.root()).and_then(|m| m.modified()).ok();
        assert_eq!(before, after, "the cache relies on appends leaving the folder date alone");
    }

    #[test]
    fn a_removed_successor_is_dropped() {
        let tmp = TempDir::new("succ4");
        tmp.write("old.jsonl", "{}\n");
        tick();
        tmp.write("new.jsonl", &cleared("worker"));
        let old = path(&tmp, "old.jsonl");
        let mut cache = Successors::default();
        assert_eq!(cache.find(&old, "worker"), Some(path(&tmp, "new.jsonl")));
        tick();
        std::fs::remove_file(tmp.path("new.jsonl")).expect("remove");
        assert_eq!(cache.find(&old, "worker"), None);
    }

    #[test]
    fn a_name_with_quotes_is_matched_as_json() {
        let tmp = TempDir::new("succ5");
        tmp.write("old.jsonl", "{}\n");
        tick();
        tmp.write("new.jsonl", &cleared("say \\\"hi\\\""));
        assert_eq!(search_successor(&path(&tmp, "old.jsonl"), "say \"hi\""), Some(path(&tmp, "new.jsonl")));
    }
}
