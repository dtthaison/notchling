use super::failure_tests::{failing_with, Failing};
use super::*;
use crate::consent_testkit::agent_caller;
use crate::job_book::Record;
use crate::vault_caller::{Processes, SystemProcesses};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

struct Clockwork {
    start: AtomicU64,
    exited: AtomicBool,
}

impl Clockwork {
    fn at(start: u64) -> Arc<Clockwork> {
        Arc::new(Clockwork {
            start: AtomicU64::new(start),
            exited: AtomicBool::new(false),
        })
    }
}

impl Processes for Clockwork {
    fn parent(&self, _pid: u32) -> Option<u32> {
        None
    }
    fn name(&self, _pid: u32) -> Option<String> {
        None
    }
    fn cwd(&self, _pid: u32) -> Option<PathBuf> {
        None
    }
    fn resolve(&self, _path: &Path) -> Option<PathBuf> {
        None
    }
    fn start_time(&self, _pid: u32) -> Option<u64> {
        Some(self.start.load(Ordering::SeqCst)).filter(|start| *start != 0)
    }
    fn exited(&self, _pid: u32) -> bool {
        self.exited.load(Ordering::SeqCst)
    }
}

fn delegate_record(started: i64) -> Record {
    Record {
        session: "parent".to_string(),
        organization: "Personal".to_string(),
        kind: "delegate".to_string(),
        tool: "agy".to_string(),
        folder: "/work/a".to_string(),
        started_ms: started,
        ended_ms: None,
        exit: None,
    }
}

fn state_of(asks: &Arc<JobAsks>, id: &str) -> String {
    let listed = asks
        .call(&agent_caller(), ops::JOBS_MINE, &json!({}))
        .unwrap();
    listed["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == id)
        .map(|j| j["state"].as_str().unwrap_or_default().to_string())
        .unwrap_or_default()
}

fn over(tag: &str, procs: Arc<Clockwork>) -> (Failing, Arc<Clockwork>) {
    let book = JobBook::in_memory().with_processes(procs.clone());
    (
        failing_with(tag, ApiError::internal("unused"), Arc::new(book)),
        procs,
    )
}

fn begin_as(asks: &Arc<JobAsks>, pid: u32) -> String {
    let mut caller = agent_caller();
    caller.pid = Some(pid);
    asks.call(
        &caller,
        ops::DELEGATE_BEGIN,
        &json!({"tool": "agy", "folder": "/work/a"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn a_pid_that_another_process_took_over_reads_as_lost() {
    let (f, procs) = over("reuse", Clockwork::at(1_000));
    let id = begin_as(&f.asks, std::process::id());
    assert_eq!(state_of(&f.asks, &id), "running");
    procs.start.store(2_000, Ordering::SeqCst);
    assert_eq!(state_of(&f.asks, &id), "lost");
}

#[test]
fn a_pid_whose_start_time_cannot_be_read_any_more_reads_as_lost() {
    let (f, procs) = over("unreadable", Clockwork::at(1_000));
    let id = begin_as(&f.asks, std::process::id());
    procs.start.store(0, Ordering::SeqCst);
    assert_eq!(state_of(&f.asks, &id), "lost");
}

#[test]
fn a_delegate_that_is_still_the_same_process_keeps_running_past_a_day() {
    let (f, _procs) = over("long", Clockwork::at(1_000));
    f.asks.book().record_with_pid(
        "d-long",
        delegate_record(now_ms() - 3 * DAY_MS),
        Some(std::process::id()),
    );
    assert_eq!(state_of(&f.asks, "d-long"), "running");
}

#[test]
fn a_delegate_with_no_process_still_ends_as_lost_after_a_day() {
    let (f, _procs) = over("nopid", Clockwork::at(1_000));
    f.asks
        .book()
        .record("d-none", delegate_record(now_ms() - 3 * DAY_MS));
    assert_eq!(state_of(&f.asks, "d-none"), "lost");
}

#[test]
fn a_book_saved_without_start_times_checks_only_that_the_process_exists() {
    let dir = std::env::temp_dir().join(format!("nl-book-nostart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("jobs.json");
    let now = now_ms();
    let old = format!(
        r#"{{"jobs":{{"d-1":{{"session":"parent","organization":"Personal","kind":"delegate","tool":"agy","folder":"/work/a","started_ms":{now}}}}},"pids":{{"d-1":{}}}}}"#,
        std::process::id()
    );
    std::fs::write(&path, old).unwrap();
    let book = JobBook::open(path).with_processes(Clockwork::at(5_000));
    let f = failing_with("legacy", ApiError::internal("unused"), Arc::new(book));
    assert_eq!(state_of(&f.asks, "d-1"), "running");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_real_end_corrects_a_job_that_was_read_as_lost_too_early() {
    let (f, _procs) = over("late-end", Clockwork::at(1_000));
    f.asks
        .book()
        .record("d-late", delegate_record(now_ms() - 3 * DAY_MS));
    assert_eq!(state_of(&f.asks, "d-late"), "lost");
    f.asks
        .call(
            &agent_caller(),
            ops::DELEGATE_END,
            &json!({"id": "d-late", "exit": 0}),
        )
        .unwrap();
    assert_eq!(state_of(&f.asks, "d-late"), "done");
}

#[test]
fn a_process_that_exited_but_is_not_reaped_reads_as_lost_though_its_start_time_matches() {
    let (f, procs) = over("zombie", Clockwork::at(1_000));
    let id = begin_as(&f.asks, std::process::id());
    assert_eq!(state_of(&f.asks, &id), "running");
    procs.exited.store(true, Ordering::SeqCst);
    assert_eq!(state_of(&f.asks, &id), "lost");
}

fn is_zombie(pid: u32) -> bool {
    let out = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim_start()
        .starts_with('Z')
}

#[test]
fn a_real_child_that_died_and_is_not_reaped_reads_as_lost() {
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = child.id();
    let book = JobBook::in_memory().with_processes(Arc::new(SystemProcesses));
    book.record_with_pid("d-zombie", delegate_record(now_ms()), Some(pid));
    let f = failing_with("real-zombie", ApiError::internal("unused"), Arc::new(book));
    assert_eq!(state_of(&f.asks, "d-zombie"), "running");
    child.kill().unwrap();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) && !is_zombie(pid) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(is_zombie(pid));
    assert_eq!(state_of(&f.asks, "d-zombie"), "lost");
    child.wait().unwrap();
}
