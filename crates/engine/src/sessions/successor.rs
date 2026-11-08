//! A job's transcript: its worktree folder and the file `/clear` started.

use std::collections::HashMap;
use std::time::SystemTime;

/// Claude Code's folder name for a working directory.
pub fn project_folder(_cwd: &str) -> String {
    todo!()
}

/// A background job's transcript path.
pub fn job_transcript(
    _link_scan_path: Option<&str>,
    _session_id: Option<&str>,
    _worktree_path: Option<&str>,
    _modified: &dyn Fn(&str) -> Option<i64>,
) -> Option<String> {
    todo!()
}

/// Finds the transcript a job moved to after `/clear`, uncached.
pub fn search_successor(_path: &str, _name: &str) -> Option<String> {
    todo!()
}

/// `search_successor` with its answers kept until the transcript folder's date changes.
#[derive(Default)]
pub struct Successors {
    cache: HashMap<String, (SystemTime, Option<String>)>,
}

impl Successors {
    pub fn find(&mut self, _path: &str, _name: &str) -> Option<String> {
        let _ = &self.cache;
        todo!()
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
