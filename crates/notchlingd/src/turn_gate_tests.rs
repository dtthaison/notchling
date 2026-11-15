use crate::acting_ops_tests::{row, setup_in, PROMPT};
use crate::outbox_courier::{ClaudeCourier, Clock, Courier};
use notchling_engine::outbox::{
    gate, Gate, New, OutboxStore, Row, Tool, TryEnd, Via, Why, TURN_SETTLE_MS,
};
use notchling_engine::sessions::Roots;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

const JOB: &str = "be50d36a";
const ID: &str = "fee7356b-b891-4f6f-b4e7-35f7093b251c";
const SETTLE: i64 = TURN_SETTLE_MS;

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("nl-turn-gate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("claude/jobs").join(JOB)).unwrap();
        std::fs::create_dir_all(dir.join("claude/sessions")).unwrap();
        Dir(dir)
    }

    fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.0.join(rel), text).unwrap();
    }

    fn roots(&self) -> Roots {
        Roots {
            main_dir: self.0.join("claude"),
            lanes_file: self.0.join("none/lanes.json"),
        }
    }

    fn transcript(&self) -> String {
        self.0
            .join("transcript.jsonl")
            .to_string_lossy()
            .into_owned()
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

fn prompt(text: &str) -> String {
    json!({"type": "user", "timestamp": "2026-11-14T22:57:14.889Z",
        "message": {"role": "user", "content": text}})
    .to_string()
}

fn job_file(dir: &Dir, state: &str, tasks: u64, kinds: &[&str]) {
    let job = json!({
        "state": state, "tempo": "blocked", "detail": "d", "needs": "Do you want it?",
        "inFlight": {"tasks": tasks, "queued": 0, "kinds": kinds, "drainableMonitors": 0},
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

fn ended(asked: &str) -> Vec<String> {
    vec![prompt("go"), assistant(asked), turn_end()]
}

fn begun() -> Vec<String> {
    vec![prompt("go"), assistant("starting")]
}

fn after_end(asked: &str, next: &str) -> Vec<String> {
    let mut all = ended(asked);
    all.push(prompt(next));
    all
}

struct Gatekeeper {
    courier: ClaudeCourier,
    row: Row,
    attached: crate::acting_ops_tests::Attaches,
    clock: Arc<AtomicI64>,
}

fn keeper(dir: &Dir) -> Gatekeeper {
    let list = json!([row(
        ID,
        json!({"job": JOB}),
        "working",
        Some(&dir.transcript())
    )]);
    let setup = setup_in(true, PROMPT, list, dir.roots(), None);
    let attached = setup.attached.clone();
    let ops = Arc::new(setup.ops);
    let clock = Arc::new(AtomicI64::new(0));
    let ticks = clock.clone();
    let now: Clock = Arc::new(move || ticks.load(Ordering::SeqCst));
    let courier = ClaudeCourier::new(ops, dir.roots(), now);
    let store = OutboxStore::in_memory_for_tests();
    let new = New {
        session: JOB.into(),
        tool: Tool::Claude,
        text: "/compact".into(),
        options: None,
        via: Via::Chat,
        clear_box: false,
    };
    let row = store.insert(&new, 0).unwrap();
    Gatekeeper {
        courier,
        row,
        attached,
        clock,
    }
}

impl Gatekeeper {
    fn at(&self, ms: i64) -> Gate {
        self.clock.store(ms, Ordering::SeqCst);
        gate(&self.row, &self.courier.facts(&self.row, ms))
    }

    fn goes_at(&self, ms: i64) -> bool {
        self.at(ms) == Gate::Go { guarded: true }
    }

    fn never_for(&self, from: i64, ticks: i64) {
        for tick in 0..ticks {
            let decided = self.at(from + tick * 1_000);
            assert!(
                !matches!(decided, Gate::Go { .. }),
                "tick {tick}: {decided:?}"
            );
        }
    }
}

#[test]
fn a_turn_that_just_started_is_waited_out() {
    let dir = Dir::new("incident");
    lines(&dir, &after_end("Do you want it?", "yes, both"));
    job_file(&dir, "blocked", 4, &["local_bash"]);
    live_file(&dir, "busy");
    let k = keeper(&dir);
    k.never_for(1_000, 90);
    assert!(k.attached.lock().unwrap().is_empty());
}

#[test]
fn it_goes_once_the_new_turn_has_ended_and_stood_for_the_settle_time() {
    let dir = Dir::new("ends");
    lines(&dir, &after_end("Do you want it?", "yes, both"));
    job_file(&dir, "blocked", 4, &["local_bash"]);
    live_file(&dir, "busy");
    let k = keeper(&dir);
    k.never_for(1_000, 5);
    let mut done = after_end("Do you want it?", "yes, both");
    done.extend([assistant("on it"), turn_end()]);
    lines(&dir, &done);
    assert!(!k.goes_at(10_000));
    assert!(
        !k.goes_at(10_000 + SETTLE - 1),
        "not before the settle time"
    );
    assert!(k.goes_at(10_000 + SETTLE));
}

#[test]
fn the_job_and_live_files_are_read_again_each_time() {
    let dir = Dir::new("fresh");
    lines(&dir, &ended("done"));
    job_file(&dir, "working", 0, &["local_bash"]);
    live_file(&dir, "busy");
    let k = keeper(&dir);
    k.never_for(1_000, 30);
    live_file(&dir, "waiting");
    job_file(&dir, "done", 3, &["local_bash"]);
    assert!(matches!(
        k.at(40_000),
        Gate::Wait {
            why: Why::Dialog,
            ..
        }
    ));
    live_file(&dir, "busy");
    assert!(!k.goes_at(41_000));
    assert!(k.goes_at(41_000 + SETTLE));
}

#[test]
fn a_slash_command_run_while_idle_does_not_keep_it_waiting() {
    let dir = Dir::new("slash");
    let local = json!({"type": "user", "isMeta": true,
        "message": {"role": "user", "content": [{"type": "text", "text": "<local-command-caveat>Caveat</local-command-caveat>"}]}})
    .to_string();
    let mut all = ended("done");
    all.push(local);
    lines(&dir, &all);
    job_file(&dir, "done", 3, &["local_bash"]);
    live_file(&dir, "busy");
    let k = keeper(&dir);
    assert!(!k.goes_at(1_000));
    assert!(k.goes_at(1_000 + SETTLE));
}

#[test]
fn a_turn_that_starts_while_the_terminal_is_attached_stops_before_the_first_key() {
    let dir = Dir::new("attached");
    lines(&dir, &ended("done"));
    job_file(&dir, "done", 3, &["local_bash"]);
    live_file(&dir, "busy");
    let k = keeper(&dir);
    assert!(!k.goes_at(1_000));
    assert!(k.goes_at(1_000 + SETTLE));
    lines(&dir, &after_end("done", "typed in the terminal"));
    let (end, tried, _) = k.courier.deliver(&k.row, true);
    assert_eq!(end, TryEnd::NotIdle);
    assert!(!tried.pasted && !tried.entered);
    assert_eq!(k.attached.lock().unwrap().len(), 1, "it did attach");
}

#[test]
fn a_blocked_job_idle_with_a_background_shell_goes_after_the_settle() {
    let dir = Dir::new("shell-direct");
    lines(&dir, &ended("Choose 1, 2, or 3"));
    job_file(&dir, "blocked", 9, &["local_bash"]);
    live_file(&dir, "shell");
    let k = keeper(&dir);
    assert!(!k.goes_at(1_000));
    assert!(k.goes_at(1_000 + SETTLE));
}

#[test]
fn a_running_turn_then_an_idle_shell_goes_after_the_settle() {
    let dir = Dir::new("shell-held");
    lines(&dir, &begun());
    job_file(&dir, "working", 9, &["local_bash"]);
    live_file(&dir, "busy");
    let k = keeper(&dir);
    k.never_for(1_000, 5);
    lines(&dir, &ended("Choose 1, 2, or 3"));
    job_file(&dir, "blocked", 9, &["local_bash"]);
    live_file(&dir, "shell");
    assert!(!k.goes_at(20_000));
    assert!(k.goes_at(20_000 + SETTLE));
}

#[test]
fn a_new_turn_under_a_shell_status_is_still_waited_out() {
    let dir = Dir::new("shell-new-turn");
    lines(&dir, &after_end("Choose 1, 2, or 3", "2"));
    job_file(&dir, "blocked", 9, &["local_bash"]);
    live_file(&dir, "shell");
    keeper(&dir).never_for(1_000, 30);
}

const AGENTS: &[&str] = &["artifact_watch", "local_agent"];

#[test]
fn a_busy_job_whose_turn_ended_while_subagents_run_goes_after_the_settle() {
    let dir = Dir::new("agents-direct");
    lines(&dir, &ended("the agents are out"));
    job_file(&dir, "working", 7, AGENTS);
    live_file(&dir, "busy");
    let k = keeper(&dir);
    assert!(!k.goes_at(1_000));
    assert!(k.goes_at(1_000 + SETTLE));
}

#[test]
fn a_turn_held_until_only_subagents_keep_the_job_busy_goes_exactly_at_the_settle() {
    let dir = Dir::new("agents-held");
    lines(&dir, &begun());
    job_file(&dir, "working", 7, AGENTS);
    live_file(&dir, "busy");
    let k = keeper(&dir);
    k.never_for(1_000, 5);
    lines(&dir, &ended("the agents are out"));
    assert!(!k.goes_at(30_000));
    assert!(!k.goes_at(30_000 + SETTLE - 1));
    assert!(k.goes_at(30_000 + SETTLE));
}

#[test]
fn a_prompt_after_the_last_turn_end_is_a_running_turn_even_with_subagents() {
    let dir = Dir::new("agents-new-turn");
    lines(&dir, &after_end("the agents are out", "and the tests?"));
    job_file(&dir, "working", 7, AGENTS);
    live_file(&dir, "busy");
    keeper(&dir).never_for(1_000, 30);
}

#[test]
fn a_turn_that_starts_inside_the_settle_time_is_waited_out() {
    let dir = Dir::new("agents-settle");
    lines(&dir, &ended("the agents are out"));
    job_file(&dir, "working", 7, AGENTS);
    live_file(&dir, "busy");
    let k = keeper(&dir);
    assert!(!k.goes_at(30_000));
    lines(
        &dir,
        &after_end("the agents are out", "typed in the terminal"),
    );
    k.never_for(30_000 + SETTLE, 30);
}

#[test]
fn a_busy_job_with_nothing_in_flight_is_the_turn_whatever_the_transcript_says() {
    let dir = Dir::new("agents-none");
    lines(&dir, &ended("the agents are out"));
    job_file(&dir, "working", 0, AGENTS);
    live_file(&dir, "busy");
    keeper(&dir).never_for(1_000, 30);
}

#[test]
fn a_kept_reading_is_dropped_when_it_stops_holding() {
    let dir = Dir::new("reset");
    lines(&dir, &ended("done"));
    job_file(&dir, "done", 3, &["local_bash"]);
    live_file(&dir, "busy");
    let k = keeper(&dir);
    assert!(!k.goes_at(1_000));
    lines(&dir, &after_end("done", "more"));
    assert!(!k.goes_at(2_000));
    lines(&dir, &ended("done"));
    assert!(!k.goes_at(4_000), "the settle starts again");
    assert!(!k.goes_at(4_000 + SETTLE - 1));
    assert!(k.goes_at(4_000 + SETTLE));
}
