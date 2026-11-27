use crate::acting::ACTING_OFF;
use crate::acting_ops_tests::{app, caller, code, sessions, setup_in, Setup};
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, Tier};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

const RUNNING: &str = include_str!("../../engine/tests/fixtures/shell-live/running-lines.txt");
const JOB: &str = "abcd1234";
const GONE: u32 = 99_999_999;

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let dir =
            std::env::temp_dir().join(format!("nl-shell-output-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("claude/jobs").join(JOB)).unwrap();
        std::fs::create_dir_all(dir.join("claude/sessions")).unwrap();
        Dir(dir)
    }

    fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.0.join(rel), text).unwrap();
    }

    fn state(&self, state: &str) {
        let job = json!({"state": state, "tempo": "active", "template": "bg", "name": "main",
            "sessionId": "be50d36a-4348-4ba6-ab58-96959399a893", "cwd": "/Users/me/app",
            "updatedAt": "2026-11-14T22:54:49.559Z"});
        self.write(&format!("claude/jobs/{JOB}/state.json"), &job.to_string());
    }

    fn live(&self, pid: u32, status: &str) {
        let live = json!({"pid": pid, "sessionId": "fee7356b-b891-4f6f-b4e7-35f7093b251c",
            "jobId": JOB, "status": status, "statusUpdatedAt": 1_794_697_000_000.0});
        self.write("claude/sessions/1.json", &live.to_string());
    }

    fn roots(&self) -> Roots {
        Roots {
            main_dir: self.0.join("claude"),
            lanes_file: self.0.join("none/lanes.json"),
        }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn screen() -> Vec<&'static str> {
    RUNNING.lines().skip(1).collect()
}

fn ask() -> Value {
    json!({"id": "job-a", "command": "npm run build"})
}

fn running(tag: &str, status: &str) -> (Dir, Setup) {
    let dir = Dir::new(tag);
    dir.state("working");
    dir.live(std::process::id(), status);
    let s = setup_in(true, &screen(), sessions(), dir.roots(), None);
    (dir, s)
}

#[test]
fn the_app_reads_the_lines_of_a_command_that_is_still_running() {
    let (_dir, s) = running("lines", "busy");
    let read = s
        .ops
        .call(&app(), ops::SESSION_SHELL_OUTPUT, &ask())
        .unwrap();
    assert_eq!(
        read,
        json!({"lines": [
            "> app@1.0.0 build",
            "> tsc -p .",
            "",
            "src/main.ts(3,7): error TS2322: Type 'string' is not assignable to type 'number'.",
            "  3 | const n: number = \"x\";"
        ]})
    );
    assert_eq!(
        s.attached.lock().unwrap().as_slice(),
        [(JOB.to_string(), Some("/lanes/b".to_string()))]
    );
    assert!(s.writes.lock().unwrap().is_empty(), "nothing typed");
}

#[test]
fn a_job_running_a_shell_command_is_read_too() {
    let (_dir, s) = running("shell", "shell");
    let read = s
        .ops
        .call(&app(), ops::SESSION_SHELL_OUTPUT, &ask())
        .unwrap();
    assert_eq!(read["lines"].as_array().map(Vec::len), Some(5));
    assert_eq!(s.attached.lock().unwrap().len(), 1);
}

#[test]
fn it_waits_for_the_switch_and_the_app() {
    let dir = Dir::new("off");
    dir.state("working");
    dir.live(std::process::id(), "busy");
    let off = setup_in(false, &screen(), sessions(), dir.roots(), None);
    let read = off.ops.call(&app(), ops::SESSION_SHELL_OUTPUT, &ask());
    assert_eq!(code(read), (ErrorCode::Forbidden, ACTING_OFF.to_string()));
    let (_dir, on) = running("agent", "busy");
    let agent = Caller {
        tier: Tier::Agent,
        ..caller(ClientKind::Cli)
    };
    let read = on.ops.call(&agent, ops::SESSION_SHELL_OUTPUT, &ask());
    assert_eq!(code(read).0, ErrorCode::Forbidden);
    assert!(on.attached.lock().unwrap().is_empty());
    assert!(off.attached.lock().unwrap().is_empty());
}

#[test]
fn a_session_that_is_not_working_is_never_attached() {
    let (_dir, s) = running("idle", "busy");
    let read = s
        .ops
        .call(
            &app(),
            ops::SESSION_SHELL_OUTPUT,
            &json!({"id": "job-idle", "command": "npm run build"}),
        )
        .unwrap();
    assert_eq!(read, json!({"lines": []}));
    assert!(
        s.attached.lock().unwrap().is_empty(),
        "an attach would wake a job that has ended"
    );
}

#[test]
fn a_job_the_cache_still_calls_working_is_not_attached_once_its_files_say_idle() {
    let (dir, s) = running("stale", "busy");
    dir.live(std::process::id(), "idle");
    let read = s
        .ops
        .call(&app(), ops::SESSION_SHELL_OUTPUT, &ask())
        .unwrap();
    assert_eq!(read, json!({"lines": []}));
    assert!(s.attached.lock().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn a_dead_process_whose_job_file_still_says_working_is_never_attached() {
    let (dir, s) = running("dead", "busy");
    dir.live(GONE, "busy");
    let read = s
        .ops
        .call(&app(), ops::SESSION_SHELL_OUTPUT, &ask())
        .unwrap();
    assert_eq!(read, json!({"lines": []}));
    assert!(
        s.attached.lock().unwrap().is_empty(),
        "an attach would restart a job whose process is gone"
    );
    std::fs::remove_file(dir.0.join("claude/sessions/1.json")).unwrap();
    let read = s
        .ops
        .call(&app(), ops::SESSION_SHELL_OUTPUT, &ask())
        .unwrap();
    assert_eq!(read, json!({"lines": []}));
    assert!(s.attached.lock().unwrap().is_empty());
}

#[test]
fn a_session_without_a_terminal_and_a_missing_command_are_refused() {
    let (_dir, s) = running("refused", "busy");
    let term = s.ops.call(
        &app(),
        ops::SESSION_SHELL_OUTPUT,
        &json!({"id": "term", "command": "ls"}),
    );
    assert_eq!(code(term).0, ErrorCode::BadRequest);
    let bare = s
        .ops
        .call(&app(), ops::SESSION_SHELL_OUTPUT, &json!({"id": "job-a"}));
    assert_eq!(code(bare).0, ErrorCode::BadRequest);
    assert!(s.attached.lock().unwrap().is_empty());
}

#[test]
fn a_command_that_is_not_on_the_screen_has_no_lines() {
    let (_dir, s) = running("absent", "busy");
    let read = s
        .ops
        .call(
            &app(),
            ops::SESSION_SHELL_OUTPUT,
            &json!({"id": "job-a", "command": "ls -la"}),
        )
        .unwrap();
    assert_eq!(read, json!({"lines": []}));
}

#[test]
fn a_read_never_waits_behind_a_reply_or_an_interrupt() {
    let (_dir, s) = running("busy-lock", "busy");
    let held = s.ops.typing_locks().for_job(JOB);
    let typing = held.lock().unwrap();
    let ops_ = Arc::new(s.ops);
    let (tx, rx) = mpsc::channel();
    let reader = ops_.clone();
    std::thread::spawn(move || {
        let _ = tx.send(reader.call(&app(), ops::SESSION_SHELL_OUTPUT, &ask()));
    });
    let read = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the read waited for the typing lock")
        .unwrap();
    assert_eq!(read, json!({"lines": [], "busy": true}));
    assert!(s.attached.lock().unwrap().is_empty());
    drop(typing);
}

#[test]
fn the_terminal_is_closed_after_the_typing_lock_is_given_back() {
    let (_dir, s) = running("teardown", "busy");
    let held = s.ops.typing_locks().for_job(JOB);
    let (closed, free) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let (seen, was_free) = (closed.clone(), free.clone());
    *s.closed.lock().unwrap() = Some(Arc::new(move || {
        was_free.store(held.try_lock().is_ok(), Ordering::SeqCst);
        seen.store(true, Ordering::SeqCst);
    }));
    s.ops
        .call(&app(), ops::SESSION_SHELL_OUTPUT, &ask())
        .unwrap();
    assert!(closed.load(Ordering::SeqCst), "the attach was closed");
    assert!(
        free.load(Ordering::SeqCst),
        "a reply or an interrupt would have waited for the close"
    );
}
