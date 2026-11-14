use crate::acting_ops_tests::{app, row, setup, setup_in, Setup, PROMPT};
use crate::held_guard::{FromFiles, Rest};
use notchling_engine::reply::held::{SETTLE_MS, START_GRACE_MS};
use notchling_engine::reply::keys::timing::{ATTACH_QUIET_MS, KEY_PAUSE_MS};
use notchling_engine::sessions::{Agent, Roots};
use notchling_local_api::ops;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const JOB: &str = "be50d36a";
const ID: &str = "fee7356b-b891-4f6f-b4e7-35f7093b251c";

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("nl-held-race-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("claude/jobs").join(JOB)).unwrap();
        std::fs::create_dir_all(dir.join("claude/sessions")).unwrap();
        Dir(dir)
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.path(rel), text).unwrap();
    }

    fn roots(&self) -> Roots {
        Roots {
            main_dir: self.path("claude"),
            lanes_file: self.path("none/lanes.json"),
        }
    }

    fn transcript(&self) -> String {
        self.path("transcript.jsonl").to_string_lossy().into_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn assistant(text: &str) -> String {
    json!({"type": "assistant", "timestamp": "2026-11-14T22:54:49.559Z",
        "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}})
    .to_string()
}

fn turn_end() -> String {
    json!({"type": "system", "subtype": "turn_duration", "durationMs": 20000,
        "timestamp": "2026-11-14T22:54:49.593Z"})
    .to_string()
}

fn prompt_line(text: &str) -> String {
    json!({"type": "user", "timestamp": "2026-11-14T22:57:14.889Z",
        "message": {"role": "user", "content": text}})
    .to_string()
}

fn job_file(dir: &Dir, state: &str, tasks: u64, detail: &str) {
    let job = json!({
        "state": state, "tempo": "blocked", "detail": detail, "needs": "Do you want it?",
        "inFlight": {"tasks": tasks, "queued": 0, "kinds": ["local_bash"], "drainableMonitors": 0},
        "template": "bg", "name": "main", "sessionId": "be50d36a-4348-4ba6-ab58-96959399a893",
        "linkScanPath": dir.transcript(), "cwd": "/Users/me/app",
        "updatedAt": "2026-11-14T22:54:49.559Z"
    });
    dir.write(&format!("claude/jobs/{JOB}/state.json"), &job.to_string());
}

fn live_file(dir: &Dir, status: &str) {
    let live = json!({"pid": std::process::id(), "sessionId": ID, "jobId": JOB,
        "status": status, "statusUpdatedAt": 1_794_697_000_000.0});
    dir.write("claude/sessions/1.json", &live.to_string());
}

fn lines(dir: &Dir, lines: &[String]) {
    dir.write("transcript.jsonl", &format!("{}\n", lines.join("\n")));
}

fn cached(dir: &Dir, group: &str, at_prompt: bool) -> Value {
    let mut r = row(ID, json!({"job": JOB}), group, Some(&dir.transcript()));
    r["atPrompt"] = json!(at_prompt);
    r
}

fn from_files(s: Setup, roots: Roots) -> Setup {
    let Setup {
        ops,
        room,
        attached,
        writes,
        deaf,
        starting,
        human,
    } = s;
    Setup {
        ops: ops.with_rest(Box::new(FromFiles::new(roots))),
        room,
        attached,
        writes,
        deaf,
        starting,
        human,
    }
}

fn session(dir: &Dir, group: &str, at_prompt: bool) -> Setup {
    let list = json!([cached(dir, group, at_prompt)]);
    let s = setup_in(true, PROMPT, list, dir.roots(), None);
    from_files(s, dir.roots())
}

fn set_row(s: &Setup, dir: &Dir, group: &str, at_prompt: bool) {
    *s.ops.sessions.lock().unwrap() = json!([cached(dir, group, at_prompt)]);
}

fn say(s: &Setup, text: &str) -> Value {
    s.ops
        .call(&app(), ops::SESSION_REPLY, &json!({"id": ID, "text": text}))
        .unwrap()
}

fn typed(s: &Setup) -> String {
    s.writes.lock().unwrap().concat()
}

#[test]
fn a_held_text_is_never_typed_into_the_turn_that_just_started() {
    let dir = Dir::new("incident");
    lines(
        &dir,
        &[prompt_line("go"), assistant("Do you want it?"), turn_end()],
    );
    job_file(&dir, "blocked", 0, "Do you want it?");
    live_file(&dir, "busy");
    let s = session(&dir, "working", false);
    assert_eq!(say(&s, "co")["state"], json!("queued"));

    lines(
        &dir,
        &[
            prompt_line("go"),
            assistant("Do you want it?"),
            turn_end(),
            prompt_line("yes, both"),
        ],
    );
    job_file(&dir, "blocked", 4, "yes, both");
    set_row(&s, &dir, "working", true);
    for tick in 1..=90 {
        s.ops.flush_held(tick * 1_000);
    }
    assert!(
        s.attached.lock().unwrap().is_empty(),
        "no terminal was attached"
    );
    assert!(typed(&s).is_empty());
    assert_eq!(s.ops.held_text(ID).as_deref(), Some("co"));
}

#[test]
fn it_goes_in_once_the_new_turn_has_ended_and_stood_so_for_the_settle_time() {
    let dir = Dir::new("ends");
    let begun = [
        prompt_line("go"),
        assistant("Do you want it?"),
        turn_end(),
        prompt_line("yes, both"),
    ];
    lines(&dir, &begun);
    job_file(&dir, "blocked", 4, "yes, both");
    live_file(&dir, "busy");
    let s = session(&dir, "working", false);
    say(&s, "co");
    set_row(&s, &dir, "working", true);
    s.ops.flush_held(1_000);
    s.ops.flush_held(5_000);
    assert!(typed(&s).is_empty());

    let mut ended = begun.to_vec();
    ended.extend([assistant("on it"), turn_end()]);
    lines(&dir, &ended);
    s.ops.flush_held(10_000);
    s.ops.flush_held(10_000 + SETTLE_MS - 1);
    assert!(typed(&s).is_empty(), "not before the settle time");
    s.ops.flush_held(10_000 + SETTLE_MS);
    assert!(typed(&s).contains("co"));
    assert_eq!(s.ops.held_text(ID), None);
}

#[test]
fn the_job_and_live_files_are_read_again_so_a_cached_calm_row_is_not_enough() {
    let dir = Dir::new("fresh");
    lines(&dir, &[prompt_line("go"), assistant("done"), turn_end()]);
    job_file(&dir, "done", 3, "done");
    live_file(&dir, "busy");
    let s = session(&dir, "working", false);
    say(&s, "co");
    set_row(&s, &dir, "completed", false);
    job_file(&dir, "working", 3, "a new turn");
    for tick in 1..=30 {
        s.ops.flush_held(tick * 1_000);
    }
    assert!(typed(&s).is_empty(), "the file says the turn is running");
    live_file(&dir, "waiting");
    job_file(&dir, "done", 3, "done");
    s.ops.flush_held(40_000);
    assert!(typed(&s).is_empty(), "a dialog is open");
    live_file(&dir, "busy");
    s.ops.flush_held(41_000);
    s.ops.flush_held(41_000 + SETTLE_MS);
    assert!(typed(&s).contains("co"));
}

#[test]
fn a_slash_command_run_while_idle_does_not_keep_held_text_waiting() {
    let dir = Dir::new("slash");
    let local = json!({"type": "user", "isMeta": true,
        "message": {"role": "user", "content": [{"type": "text", "text": "<local-command-caveat>Caveat</local-command-caveat>"}]}})
    .to_string();
    lines(
        &dir,
        &[prompt_line("go"), assistant("done"), turn_end(), local],
    );
    job_file(&dir, "done", 3, "done");
    live_file(&dir, "busy");
    let s = session(&dir, "working", false);
    say(&s, "co");
    set_row(&s, &dir, "working", true);
    s.ops.flush_held(1_000);
    s.ops.flush_held(1_000 + SETTLE_MS);
    assert!(typed(&s).contains("co"));
}

struct Counting {
    calls: AtomicUsize,
    trip_at: usize,
    asked: Arc<AtomicUsize>,
}

impl Rest for Counting {
    fn fresh(&self, agent: &Agent) -> Option<Agent> {
        Some(agent.clone())
    }
    fn restarted(&self, _agent: &Agent, _now_ms: i64) -> bool {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.calls.fetch_add(1, Ordering::SeqCst) == self.trip_at
    }
}

fn with_rest(s: Setup, rest: Counting) -> Setup {
    let Setup {
        ops,
        room,
        attached,
        writes,
        deaf,
        starting,
        human,
    } = s;
    Setup {
        ops: ops.with_rest(Box::new(rest)),
        room,
        attached,
        writes,
        deaf,
        starting,
        human,
    }
}

fn counting(s: Setup, trip_at: usize) -> (Setup, Arc<AtomicUsize>) {
    let asked = Arc::new(AtomicUsize::new(0));
    let rest = Counting {
        calls: AtomicUsize::new(0),
        trip_at,
        asked: asked.clone(),
    };
    (with_rest(s, rest), asked)
}

fn working_at_prompt(s: &Setup) {
    let mut list = s.ops.sessions.lock().unwrap();
    for r in list.as_array_mut().unwrap() {
        if r["id"] == json!("job-a") {
            r["atPrompt"] = json!(true);
        }
    }
}

#[test]
fn a_turn_that_starts_while_the_terminal_is_attached_stops_the_flush_before_the_first_key() {
    let (s, asked) = counting(setup(true, PROMPT, crate::acting_ops_tests::sessions()), 2);
    say_job_a(&s, "two");
    working_at_prompt(&s);
    s.ops.flush_held(1_000);
    s.ops.flush_held(1_000 + SETTLE_MS);
    assert_eq!(
        asked.load(Ordering::SeqCst),
        3,
        "two readings and the guard"
    );
    assert_eq!(s.attached.lock().unwrap().len(), 1, "it did attach");
    assert!(typed(&s).is_empty(), "not one key");
    assert_eq!(s.ops.held_text("job-a").as_deref(), Some("two"));
    let later = 1_000 + SETTLE_MS + START_GRACE_MS;
    s.ops.flush_held(later);
    s.ops.flush_held(later + SETTLE_MS);
    assert!(
        typed(&s).contains("two"),
        "it goes in once the turn is over"
    );
    assert_eq!(s.ops.held_text("job-a"), None);
}

fn say_job_a(s: &Setup, text: &str) -> Value {
    s.ops
        .call(
            &app(),
            ops::SESSION_REPLY,
            &json!({"id": "job-a", "text": text}),
        )
        .unwrap()
}

#[test]
fn a_direct_reply_to_a_session_whose_new_turn_the_job_file_has_not_caught_up_with_is_held() {
    let (s, asked) = counting(setup(true, PROMPT, crate::acting_ops_tests::sessions()), 0);
    working_at_prompt(&s);
    let out = say_job_a(&s, "hi");
    assert_eq!(out["state"], json!("queued"));
    assert!(typed(&s).is_empty());
    assert_eq!(asked.load(Ordering::SeqCst), 1);
}

#[test]
fn a_direct_reply_to_an_idle_session_is_typed_at_once_without_reading_a_file() {
    let (s, asked) = counting(setup(true, PROMPT, crate::acting_ops_tests::sessions()), 0);
    let out = s
        .ops
        .call(
            &app(),
            ops::SESSION_REPLY,
            &json!({"id": "job-idle", "text": "go"}),
        )
        .unwrap();
    assert_eq!(out, json!({"sent": true}));
    assert!(typed(&s).contains("go"));
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}

#[test]
fn a_direct_reply_to_a_session_at_its_prompt_with_tasks_running_is_typed_at_once() {
    let (s, asked) = counting(
        setup(true, PROMPT, crate::acting_ops_tests::sessions()),
        usize::MAX,
    );
    working_at_prompt(&s);
    let out = say_job_a(&s, "hi");
    assert_eq!(out, json!({"sent": true}));
    assert!(typed(&s).contains("hi"));
    assert_eq!(asked.load(Ordering::SeqCst), 1);
}

const DRAFT: &[&str] = &[
    "────────────────────────────────────────",
    "❯ my own draft",
    "────────────────────────────────────────",
    "  ? for shortcuts",
];

fn at_rest(s: &Setup) {
    let mut list = s.ops.sessions.lock().unwrap();
    for r in list.as_array_mut().unwrap() {
        if r["id"] == json!("job-a") {
            r["group"] = json!("completed");
        }
    }
}

#[test]
fn a_flush_never_types_over_a_draft_in_the_terminals_box_and_tries_again_later() {
    let s = setup(true, DRAFT, crate::acting_ops_tests::sessions());
    say_job_a(&s, "two");
    at_rest(&s);
    s.ops.flush_held(1_000);
    s.ops.flush_held(1_000 + SETTLE_MS);
    assert_eq!(s.attached.lock().unwrap().len(), 1);
    assert!(typed(&s).is_empty(), "not even the clear keys");
    assert_eq!(s.ops.held_text("job-a").as_deref(), Some("two"));
    let later = 1_000 + SETTLE_MS + START_GRACE_MS;
    s.ops.flush_held(later);
    s.ops.flush_held(later + SETTLE_MS);
    assert_eq!(
        s.attached.lock().unwrap().len(),
        2,
        "tried again, not at once"
    );
    assert!(typed(&s).is_empty());
    assert_eq!(s.ops.held_text("job-a").as_deref(), Some("two"));
}

#[test]
fn text_a_person_types_beside_the_flush_is_never_sent_with_it_and_the_flush_is_not_repeated() {
    let s = setup(true, PROMPT, crate::acting_ops_tests::sessions());
    s.human
        .lock()
        .unwrap()
        .push((ATTACH_QUIET_MS + KEY_PAUSE_MS + 10, "x".to_string()));
    say_job_a(&s, "two");
    at_rest(&s);
    s.ops.flush_held(1_000);
    s.ops.flush_held(1_000 + SETTLE_MS);
    let written = s.writes.lock().unwrap().clone();
    assert_eq!(
        written.len(),
        2,
        "the clear and the paste, no Enter: {written:?}"
    );
    assert!(written[1].contains("two"));
    assert_eq!(
        s.ops.held_text("job-a"),
        None,
        "it may be in their box: not kept"
    );
    s.human.lock().unwrap().clear();
    let later = 1_000 + SETTLE_MS + START_GRACE_MS * 3;
    s.ops.flush_held(later);
    s.ops.flush_held(later + SETTLE_MS);
    assert_eq!(s.writes.lock().unwrap().len(), 2, "nothing was typed again");
}
