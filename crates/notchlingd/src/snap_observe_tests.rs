use super::{git_locked, main_ids, modified_ms, observe, Known, Probes};
use crate::limits_ops_tests::TempDir;
use notchling_engine::cluster::snapshot::SessionState;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

fn agent(id: &str, group: AgentGroup) -> Agent {
    Agent {
        id: id.to_string(),
        name: id.to_string(),
        project: "repo".to_string(),
        cwd: format!("/work/{id}"),
        group,
        label: "x".to_string(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job("ab12cd34".to_string()),
        suggested_reply: None,
        transcript_path: Some(format!("/t/{id}.jsonl")),
        host: "Background".to_string(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: None,
    }
}

fn ids(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

fn seen(
    rows: &[Agent],
    main: &[&str],
    known: &mut BTreeMap<String, Known>,
    locked: &[&str],
) -> notchling_engine::cluster::snapshot::Observed {
    let locked: Vec<String> = locked.iter().map(|l| l.to_string()).collect();
    let probes = Probes {
        locked: &|path: &Path| locked.iter().any(|l| path == Path::new(l)),
        written: &|path: &Path| Some(path.as_os_str().len() as i64),
    };
    observe(rows, &ids(main), known, 1_000, &probes)
}

#[test]
fn states_follow_the_session_list() {
    let rows = [
        agent("a", AgentGroup::Working),
        agent("b", AgentGroup::Completed),
        agent("d", AgentGroup::NeedsInput),
    ];
    let mut known = BTreeMap::new();
    let now = seen(&rows, &["a", "b", "c", "d"], &mut known, &[]);
    assert_eq!(now.now_ms, 1_000);
    assert_eq!(now.session_state["a"], SessionState::Working);
    assert_eq!(now.session_state["b"], SessionState::Idle);
    assert_eq!(now.session_state["d"], SessionState::Working);
    assert_eq!(now.session_state["c"], SessionState::Ended);
}

#[test]
fn sessions_outside_main_are_left_out() {
    let rows = [
        agent("a", AgentGroup::Working),
        agent("x", AgentGroup::Working),
    ];
    let mut known = BTreeMap::new();
    let now = seen(&rows, &["a"], &mut known, &[]);
    assert_eq!(now.session_state.len(), 1);
    assert!(!now.git_locks.contains_key("x"));
    assert!(!known.contains_key("x"));
}

#[test]
fn locks_and_writes_are_read_from_the_sessions_folder_and_transcript() {
    let rows = [agent("a", AgentGroup::Completed)];
    let mut known = BTreeMap::new();
    let now = seen(&rows, &["a"], &mut known, &["/work/a"]);
    assert!(now.git_locks["a"]);
    assert_eq!(now.last_write_ms["a"], "/t/a.jsonl".len() as i64);
}

#[test]
fn an_ended_session_keeps_its_last_known_folder() {
    let mut known = BTreeMap::new();
    let rows = [agent("a", AgentGroup::Working)];
    seen(&rows, &["a"], &mut known, &["/work/a"]);
    let gone = seen(&[], &["a"], &mut known, &["/work/a"]);
    assert_eq!(gone.session_state["a"], SessionState::Ended);
    assert!(gone.git_locks["a"]);
    assert!(gone.last_write_ms.contains_key("a"));
    let dropped = seen(&[], &[], &mut known, &[]);
    assert!(dropped.session_state.is_empty());
    assert!(known.is_empty());
}

#[test]
fn main_ids_come_from_the_status() {
    let status = json!({"main": ["a", "b"], "me": "x"});
    assert_eq!(main_ids(&status), ids(&["a", "b"]));
    assert!(main_ids(&json!({})).is_empty());
}

fn repo(t: &TempDir) -> std::path::PathBuf {
    let root = t.path("repo");
    fs::create_dir_all(root.join(".git/refs/heads")).unwrap();
    fs::create_dir_all(root.join("src/deep")).unwrap();
    fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    root
}

#[test]
fn no_lock_means_not_locked() {
    let t = TempDir::new("snap-observe-none");
    let root = repo(&t);
    assert!(!git_locked(&root));
    assert!(!git_locked(&root.join("src/deep")));
    assert!(!git_locked(&t.path("nowhere")));
}

#[test]
fn an_index_or_head_lock_blocks() {
    let t = TempDir::new("snap-observe-index");
    let root = repo(&t);
    fs::write(root.join(".git/index.lock"), "").unwrap();
    assert!(git_locked(&root));
    assert!(git_locked(&root.join("src/deep")));
    fs::remove_file(root.join(".git/index.lock")).unwrap();
    fs::write(root.join(".git/HEAD.lock"), "").unwrap();
    assert!(git_locked(&root));
}

#[test]
fn a_ref_lock_blocks() {
    let t = TempDir::new("snap-observe-ref");
    let root = repo(&t);
    fs::create_dir_all(root.join(".git/refs/remotes/origin")).unwrap();
    fs::write(root.join(".git/refs/remotes/origin/main.lock"), "").unwrap();
    assert!(git_locked(&root));
    fs::remove_file(root.join(".git/refs/remotes/origin/main.lock")).unwrap();
    fs::write(root.join(".git/refs/heads/topic.lock"), "").unwrap();
    assert!(git_locked(&root));
}

#[test]
fn a_linked_worktree_reads_its_own_and_the_common_folder() {
    let t = TempDir::new("snap-observe-worktree");
    let root = repo(&t);
    let admin = root.join(".git/worktrees/w");
    fs::create_dir_all(&admin).unwrap();
    fs::write(admin.join("commondir"), "../..\n").unwrap();
    let work = t.path("w");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
    assert!(!git_locked(&work));
    fs::write(admin.join("index.lock"), "").unwrap();
    assert!(git_locked(&work));
    fs::remove_file(admin.join("index.lock")).unwrap();
    fs::write(root.join(".git/refs/heads/x.lock"), "").unwrap();
    assert!(git_locked(&work));
}

fn listing(dir: &Path) -> Vec<(String, u64, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap().flatten() {
            let meta = fs::symlink_metadata(e.path()).unwrap();
            out.push((
                e.path().display().to_string(),
                meta.len(),
                meta.modified().unwrap(),
            ));
            if meta.is_dir() {
                stack.push(e.path());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn looking_for_locks_changes_nothing() {
    let t = TempDir::new("snap-observe-readonly");
    let root = repo(&t);
    fs::write(root.join(".git/index.lock"), "").unwrap();
    let before = listing(&root);
    assert!(git_locked(&root));
    assert!(!git_locked(&t.path("nowhere")));
    assert_eq!(before, listing(&root));
}

#[test]
fn modified_ms_reports_a_files_write_time() {
    let t = TempDir::new("snap-observe-mtime");
    t.write("a.jsonl", "x");
    let file = t.path("a.jsonl");
    let at = modified_ms(&file).unwrap();
    let now = crate::acting_ops::now_ms();
    assert!((now - at).abs() < 60_000);
    assert!(modified_ms(&t.path("missing")).is_none());
}
