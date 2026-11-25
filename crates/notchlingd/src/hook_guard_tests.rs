use crate::hook_guard::{bounded, process, run_in};
use notchling_engine::guard::policy;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const NOW: f64 = 1_800_000_000.0;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "nl-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        TempDir(dir)
    }

    fn root(&self) -> &Path {
        &self.0
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.path(rel);
        std::fs::write(&path, text).expect("write");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn queued(dir: &TempDir) -> Vec<String> {
    let mut paths: Vec<_> = std::fs::read_dir(dir.path("spool"))
        .map(|e| e.flatten().map(|e| e.path()).collect::<Vec<_>>())
        .unwrap_or_default();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect()
}

fn payload(command: &str) -> Vec<u8> {
    json!({
        "hook_event_name": "PreToolUse",
        "session_id": "sess-1",
        "tool_name": "Bash",
        "tool_use_id": "toolu_1",
        "cwd": "/work",
        "tool_input": {"command": command}
    })
    .to_string()
    .into_bytes()
}

#[test]
fn work_that_finishes_in_time_is_returned() {
    assert_eq!(bounded(|| 7, Duration::from_secs(2)), Some(7));
}

#[test]
fn work_past_its_deadline_is_given_up_on() {
    let started = Instant::now();
    let out = bounded(
        || {
            std::thread::sleep(Duration::from_secs(5));
            1
        },
        Duration::from_millis(50),
    );
    assert_eq!(out, None);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn work_that_panics_is_given_up_on() {
    let out: Option<u8> = bounded(|| panic!("boom"), Duration::from_secs(2));
    assert_eq!(out, None);
}

#[test]
fn a_guard_on_a_log_only_policy_queues_what_it_would_have_done() {
    let dir = TempDir::new("hook-guard");
    policy::write(dir.root(), 5).expect("policy");
    let out = process(
        payload("sleep 60"),
        dir.root().to_path_buf(),
        NOW,
        Duration::from_secs(2),
    );
    assert_eq!(out.map(|w| w.len()), Some(1));
    assert_eq!(queued(&dir).len(), 1);
}

#[test]
fn the_hook_passes_when_there_is_no_input_or_nothing_to_say() {
    let dir = TempDir::new("hook-guard");
    assert_eq!(run_in(dir.root(), |_| None), 0);
    assert_eq!(run_in(dir.root(), |_| Some(Vec::new())), 0);
    assert_eq!(run_in(dir.root(), |_| Some(b"garbage".to_vec())), 0);
    assert_eq!(run_in(dir.root(), |_| Some(payload("sleep 60"))), 0);
    assert!(queued(&dir).is_empty());
}

#[test]
fn the_hook_passes_when_the_data_folder_is_unusable() {
    let dir = TempDir::new("hook-guard");
    let file = dir.write("not-a-folder", "x");
    policy::write(dir.root(), 5).expect("policy");
    assert_eq!(run_in(&file, |_| Some(payload("sleep 60"))), 0);
}

#[test]
fn the_hook_queues_and_still_passes() {
    let dir = TempDir::new("hook-guard");
    policy::write(dir.root(), 5).expect("policy");
    assert_eq!(run_in(dir.root(), |_| Some(payload("sleep 60"))), 0);
    let rows = queued(&dir).concat();
    assert_eq!(rows.lines().count(), 1);
    assert!(!rows.contains("sleep"));
}

#[test]
fn a_late_input_is_dropped_without_waiting_or_recording() {
    let dir = TempDir::new("hook-guard");
    policy::write(dir.root(), 5).expect("policy");
    let started = Instant::now();
    let code = run_in(dir.root(), |_| {
        std::thread::sleep(Duration::from_millis(400));
        Some(payload("sleep 60"))
    });
    assert_eq!(code, 0);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(queued(&dir).is_empty());
}

#[test]
fn the_read_is_given_the_same_deadline_as_the_work() {
    let dir = TempDir::new("hook-guard");
    let asked = std::sync::Mutex::new(None);
    run_in(dir.root(), |limit| {
        *asked.lock().unwrap() = Some(limit);
        None
    });
    assert_eq!(*asked.lock().unwrap(), Some(crate::hook_guard::DEADLINE));
}
