use crate::acting_ops_tests::{app, row, setup, setup_in, Setup, PROMPT};
use crate::turn_gate::{FromFiles, Rest};
use notchling_engine::reply::held::{SETTLE_MS, START_GRACE_MS};
use notchling_engine::sessions::{Agent, Roots};
use notchling_local_api::ops;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const JOB: &str = "be50d36a";
const ID: &str = "fee7356b-b891-4f6f-b4e7-35f7093b251c";

pub(crate) struct Dir(PathBuf);

impl Dir {
    pub(crate) fn new(tag: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("nl-held-race-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("claude/jobs").join(JOB)).unwrap();
        std::fs::create_dir_all(dir.join("claude/sessions")).unwrap();
        Dir(dir)
    }

    pub(crate) fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    pub(crate) fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.path(rel), text).unwrap();
    }

    pub(crate) fn roots(&self) -> Roots {
        Roots {
            main_dir: self.path("claude"),
            lanes_file: self.path("none/lanes.json"),
        }
    }

    pub(crate) fn transcript(&self) -> String {
        self.path("transcript.jsonl").to_string_lossy().into_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn assistant(text: &str) -> String {
    json!({"type": "assistant", "timestamp": "2026-11-14T22:54:49.559Z",
        "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}})
    .to_string()
}

pub(crate) fn turn_end() -> String {
    json!({"type": "system", "subtype": "turn_duration", "durationMs": 20000,
        "timestamp": "2026-11-14T22:54:49.593Z"})
    .to_string()
}

pub(crate) fn prompt_line(text: &str) -> String {
    json!({"type": "user", "timestamp": "2026-11-14T22:57:14.889Z",
        "message": {"role": "user", "content": text}})
    .to_string()
}

pub(crate) fn job_file(dir: &Dir, state: &str, tasks: u64, detail: &str) {
    let job = json!({
        "state": state, "tempo": "blocked", "detail": detail, "needs": "Do you want it?",
        "inFlight": {"tasks": tasks, "queued": 0, "kinds": ["local_bash"], "drainableMonitors": 0},
        "template": "bg", "name": "main", "sessionId": "be50d36a-4348-4ba6-ab58-96959399a893",
        "linkScanPath": dir.transcript(), "cwd": "/Users/me/app",
        "updatedAt": "2026-11-14T22:54:49.559Z"
    });
    dir.write(&format!("claude/jobs/{JOB}/state.json"), &job.to_string());
}

pub(crate) fn live_file(dir: &Dir, status: &str) {
    let live = json!({"pid": std::process::id(), "sessionId": ID, "jobId": JOB,
        "status": status, "statusUpdatedAt": 1_794_697_000_000.0});
    dir.write("claude/sessions/1.json", &live.to_string());
}

pub(crate) fn lines(dir: &Dir, lines: &[String]) {
    dir.write("transcript.jsonl", &format!("{}\n", lines.join("\n")));
}

pub(crate) fn cached(dir: &Dir, group: &str, at_prompt: bool) -> Value {
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
        screen,
    } = s;
    Setup {
        ops: ops.with_rest(Box::new(FromFiles::new(roots))),
        room,
        attached,
        writes,
        deaf,
        starting,
        human,
        screen,
    }
}

pub(crate) fn session(dir: &Dir, group: &str, at_prompt: bool) -> Setup {
    let list = json!([cached(dir, group, at_prompt)]);
    let s = setup_in(true, PROMPT, list, dir.roots(), None);
    from_files(s, dir.roots())
}

pub(crate) fn set_row(s: &Setup, dir: &Dir, group: &str, at_prompt: bool) {
    *s.ops.sessions.lock().unwrap() = json!([cached(dir, group, at_prompt)]);
}

pub(crate) fn say(s: &Setup, text: &str) -> Value {
    s.ops
        .call(&app(), ops::SESSION_REPLY, &json!({"id": ID, "text": text}))
        .unwrap()
}

pub(crate) fn typed(s: &Setup) -> String {
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
    job_file(&dir, "working", 0, "a new turn");
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
    fn delivered(&self, _agent: &Agent, _text: &str, _since_ms: i64) -> Option<bool> {
        Some(false)
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
        screen,
    } = s;
    Setup {
        ops: ops.with_rest(Box::new(rest)),
        room,
        attached,
        writes,
        deaf,
        starting,
        human,
        screen,
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

fn row_from_files(dir: &Dir) -> Value {
    let agent = notchling_engine::sessions::fresh::job_row(
        &dir.roots(),
        &notchling_engine::sessions::Probes::from_proc(),
        JOB,
    )
    .expect("the job's row");
    let mut row = serde_json::to_value(agent).unwrap();
    row["id"] = json!(ID);
    row["transcriptPath"] = json!(dir.transcript());
    row
}

fn question_turn_over(dir: &Dir) {
    lines(
        dir,
        &[
            prompt_line("go"),
            assistant("Choose 1, 2, or 3"),
            turn_end(),
        ],
    );
}

#[test]
fn a_reply_to_a_blocked_job_idle_with_a_background_shell_is_typed_at_once() {
    let dir = Dir::new("shell-direct");
    question_turn_over(&dir);
    job_file(&dir, "blocked", 9, "Choose 1, 2, or 3");
    live_file(&dir, "shell");
    let list = json!([row_from_files(&dir)]);
    let s = from_files(setup_in(true, PROMPT, list, dir.roots(), None), dir.roots());
    assert_eq!(say(&s, "2"), json!({"sent": true}));
    assert!(typed(&s).contains('2'));
    assert_eq!(s.ops.held_text(ID), None);
}

#[test]
fn a_reply_held_while_the_turn_ran_goes_in_when_the_session_is_idle_with_a_background_shell() {
    let dir = Dir::new("shell-held");
    lines(&dir, &[prompt_line("go"), assistant("working")]);
    job_file(&dir, "working", 9, "working");
    live_file(&dir, "busy");
    let list = json!([row_from_files(&dir)]);
    let s = from_files(setup_in(true, PROMPT, list, dir.roots(), None), dir.roots());
    assert_eq!(say(&s, "2")["state"], json!("queued"));
    s.ops.flush_held(1_000);
    s.ops.flush_held(1_000 + SETTLE_MS);
    assert!(typed(&s).is_empty(), "the turn is running");

    question_turn_over(&dir);
    job_file(&dir, "blocked", 9, "Choose 1, 2, or 3");
    live_file(&dir, "shell");
    *s.ops.sessions.lock().unwrap() = json!([row_from_files(&dir)]);
    s.ops.flush_held(20_000);
    s.ops.flush_held(20_000 + SETTLE_MS);
    assert!(typed(&s).contains('2'));
    assert_eq!(s.ops.held_text(ID), None);
}

#[test]
fn a_new_turn_the_job_file_has_not_caught_up_with_is_still_waited_out_under_a_shell_status() {
    let dir = Dir::new("shell-new-turn");
    lines(
        &dir,
        &[
            prompt_line("go"),
            assistant("Choose 1, 2, or 3"),
            turn_end(),
            prompt_line("2"),
        ],
    );
    job_file(&dir, "blocked", 9, "Choose 1, 2, or 3");
    live_file(&dir, "shell");
    let list = json!([row_from_files(&dir)]);
    let s = from_files(setup_in(true, PROMPT, list, dir.roots(), None), dir.roots());
    assert_eq!(say(&s, "3")["state"], json!("queued"));
    for tick in 1..=30 {
        s.ops.flush_held(tick * 1_000);
    }
    assert!(typed(&s).is_empty());
    assert_eq!(s.ops.held_text(ID).as_deref(), Some("3"));
}

fn subagents_job_file(dir: &Dir, tasks: u64) {
    let job = json!({
        "state": "working", "tempo": "active", "detail": "waiting on the agents",
        "inFlight": {"tasks": tasks, "queued": 0, "kinds": ["artifact_watch", "local_agent"],
            "drainableMonitors": 1},
        "template": "bg", "name": "main", "sessionId": "be50d36a-4348-4ba6-ab58-96959399a893",
        "linkScanPath": dir.transcript(), "cwd": "/Users/me/app",
        "updatedAt": "2026-11-15T03:11:00.000Z"
    });
    dir.write(&format!("claude/jobs/{JOB}/state.json"), &job.to_string());
}

fn subagents_session(dir: &Dir) -> Setup {
    let list = json!([row_from_files(dir)]);
    from_files(setup_in(true, PROMPT, list, dir.roots(), None), dir.roots())
}

fn turn_over_with_subagents(dir: &Dir) {
    lines(
        dir,
        &[
            prompt_line("go"),
            assistant("the agents are out"),
            turn_end(),
        ],
    );
}

#[test]
fn a_reply_to_a_busy_job_whose_turn_ended_while_subagents_run_is_typed_at_once() {
    let dir = Dir::new("agents-direct");
    turn_over_with_subagents(&dir);
    subagents_job_file(&dir, 7);
    live_file(&dir, "busy");
    let s = subagents_session(&dir);
    assert_eq!(say(&s, "go on"), json!({"sent": true}));
    assert!(typed(&s).contains("go on"));
    assert_eq!(s.ops.held_text(ID), None);
}

#[test]
fn a_reply_held_in_a_turn_goes_in_after_the_settle_once_only_subagents_keep_the_job_busy() {
    let dir = Dir::new("agents-held");
    lines(&dir, &[prompt_line("go"), assistant("starting")]);
    subagents_job_file(&dir, 7);
    live_file(&dir, "busy");
    let s = subagents_session(&dir);
    assert_eq!(say(&s, "go on")["state"], json!("queued"));
    s.ops.flush_held(1_000);
    s.ops.flush_held(1_000 + SETTLE_MS);
    assert!(typed(&s).is_empty(), "the turn is running");

    turn_over_with_subagents(&dir);
    *s.ops.sessions.lock().unwrap() = json!([row_from_files(&dir)]);
    s.ops.flush_held(30_000);
    s.ops.flush_held(30_000 + SETTLE_MS - 1);
    assert!(typed(&s).is_empty(), "not before the settle time");
    s.ops.flush_held(30_000 + SETTLE_MS);
    assert!(typed(&s).contains("go on"));
    assert_eq!(s.ops.held_text(ID), None);
}

#[test]
fn a_prompt_after_the_last_turn_end_is_a_running_turn_even_with_subagents_in_flight() {
    let dir = Dir::new("agents-new-turn");
    lines(
        &dir,
        &[
            prompt_line("go"),
            assistant("the agents are out"),
            turn_end(),
            prompt_line("and the tests?"),
        ],
    );
    subagents_job_file(&dir, 7);
    live_file(&dir, "busy");
    let s = subagents_session(&dir);
    assert_eq!(say(&s, "also this")["state"], json!("queued"));
    for tick in 1..=30 {
        s.ops.flush_held(tick * 1_000);
    }
    assert!(typed(&s).is_empty());
    assert!(s.attached.lock().unwrap().is_empty());
    assert_eq!(s.ops.held_text(ID).as_deref(), Some("also this"));
}

#[test]
fn a_turn_that_starts_inside_the_settle_time_is_waited_out_under_subagents_too() {
    let dir = Dir::new("agents-settle");
    lines(&dir, &[prompt_line("go"), assistant("starting")]);
    subagents_job_file(&dir, 7);
    live_file(&dir, "busy");
    let s = subagents_session(&dir);
    assert_eq!(say(&s, "go on")["state"], json!("queued"));
    turn_over_with_subagents(&dir);
    *s.ops.sessions.lock().unwrap() = json!([row_from_files(&dir)]);
    s.ops.flush_held(30_000);
    lines(
        &dir,
        &[
            prompt_line("go"),
            assistant("the agents are out"),
            turn_end(),
            prompt_line("typed in the terminal"),
        ],
    );
    for tick in 0..30 {
        s.ops.flush_held(30_000 + SETTLE_MS + tick * 1_000);
    }
    assert!(typed(&s).is_empty(), "the new turn is waited out");
    assert_eq!(s.ops.held_text(ID).as_deref(), Some("go on"));
}

#[test]
fn a_busy_job_with_nothing_in_flight_is_still_the_turn_whatever_the_transcript_says() {
    let dir = Dir::new("agents-none");
    turn_over_with_subagents(&dir);
    subagents_job_file(&dir, 0);
    live_file(&dir, "busy");
    let s = subagents_session(&dir);
    assert_eq!(say(&s, "go on")["state"], json!("queued"));
    for tick in 1..=30 {
        s.ops.flush_held(tick * 1_000);
    }
    assert!(typed(&s).is_empty());
    assert_eq!(s.ops.held_text(ID).as_deref(), Some("go on"));
}
