use super::*;
use crate::outbox_courier::Courier;
use crate::outbox_ops::OutboxOps;
use crate::outbox_postman::{Couriers, Postmen};
use crate::outbox_testkit::{rig, Rig, IDLE, T0};
use notchling_engine::bridge::net::files::iso8601;
use notchling_engine::outbox::{New, OutboxStore, State, Tool, Via, Why};
use notchling_engine::sessions::Roots;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Mutex;

static FOLDERS: AtomicUsize = AtomicUsize::new(0);

struct Folder(PathBuf);

impl Folder {
    fn new(tag: &str) -> Folder {
        let n = FOLDERS.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("nl-outbox-verify-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Folder(dir)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type Write = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Writes {
    at: Mutex<Vec<(i64, Write)>>,
}

struct FakePause {
    clock: Arc<AtomicI64>,
    writes: Arc<Writes>,
}

impl FakePause {
    fn run_until(&self, deadline: i64, first_only: bool) {
        loop {
            let next = {
                let mut list = self.writes.at.lock().unwrap();
                let soonest = list
                    .iter()
                    .enumerate()
                    .filter(|(_, (at, _))| *at <= deadline)
                    .min_by_key(|(_, (at, _))| *at)
                    .map(|(n, _)| n);
                soonest.map(|n| list.remove(n))
            };
            match next {
                Some((at, write)) => {
                    self.clock.fetch_max(at, Ordering::SeqCst);
                    write();
                    if first_only {
                        return;
                    }
                }
                None => {
                    self.clock.fetch_max(deadline, Ordering::SeqCst);
                    return;
                }
            }
        }
    }
}

struct FakeWaits<'a>(&'a FakePause);

impl Waits for FakeWaits<'_> {
    fn next(&mut self, deadline_ms: i64) {
        self.0.run_until(deadline_ms, true);
    }
}

impl Pause for FakePause {
    fn watch(&self, _file: &Path, _any_jsonl: bool) -> Box<dyn Waits + '_> {
        Box::new(FakeWaits(self))
    }

    fn sleep(&self, deadline_ms: i64) {
        self.run_until(deadline_ms, false);
    }
}

struct Env {
    r: Rig,
    dir: Folder,
    path: PathBuf,
    writes: Arc<Writes>,
    verifier: Arc<TranscriptVerifier>,
    postmen: Postmen,
}

fn stamp(ms: i64) -> String {
    iso8601(T0 + ms)
}

fn enqueue(ms: i64, text: &str) -> String {
    json!({"type": "queue-operation", "operation": "enqueue", "timestamp": stamp(ms),
        "sessionId": "s-1", "content": text})
    .to_string()
}

fn operation(ms: i64, op: &str) -> String {
    json!({"type": "queue-operation", "operation": op, "timestamp": stamp(ms), "sessionId": "s-1"})
        .to_string()
}

fn remove(ms: i64, text: &str) -> String {
    json!({"type": "queue-operation", "operation": "remove", "timestamp": stamp(ms),
        "sessionId": "s-1", "content": text})
    .to_string()
}

fn user(ms: i64, text: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": text}, "uuid": "u",
        "timestamp": stamp(ms), "sessionId": "s-1", "cwd": "/work/demo"})
    .to_string()
}

fn queued_command(ms: i64, text: &str) -> String {
    json!({"type": "attachment", "attachment": {"type": "queued_command", "prompt": text,
        "commandMode": "prompt"}, "uuid": "x", "timestamp": stamp(ms), "sessionId": "s-1"})
    .to_string()
}

fn append(path: &Path, lines: &[String]) {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    for line in lines {
        writeln!(file, "{line}").unwrap();
    }
}

fn env() -> Env {
    let r = rig();
    let dir = Folder::new("env");
    let path = dir.0.join("idle.jsonl");
    append(&path, &[operation(-60_000, "dequeue")]);
    let mut agent = r.files.row(IDLE).unwrap();
    agent.transcript_path = Some(path.to_string_lossy().into_owned());
    agent.name = "worker".into();
    r.files.put(IDLE, agent);
    let writes = Arc::new(Writes::default());
    let ticks = r.clock.clone();
    let clock: Clock = Arc::new(move || ticks.load(Ordering::SeqCst));
    let pause = Arc::new(FakePause {
        clock: r.clock.clone(),
        writes: writes.clone(),
    });
    let verifier = Arc::new(TranscriptVerifier::with_parts(
        r.files.clone(),
        clock.clone(),
        pause,
        r.safe.clone(),
    ));
    let postmen = Postmen::build(
        r.ops.clone(),
        Couriers::claude_only(r.courier.clone()),
        verifier.clone(),
        r.acting_switch(),
        clock,
    );
    Env {
        r,
        dir,
        path,
        writes,
        verifier,
        postmen,
    }
}

impl Env {
    fn later(&self, ms: i64, lines: Vec<String>) {
        let path = self.path.clone();
        let write: Write = Box::new(move || append(&path, &lines));
        self.writes.at.lock().unwrap().push((T0 + ms, write));
    }

    fn now(&self, ms: i64, lines: &[String]) {
        append(&self.path, lines);
        self.r.at(T0 + ms);
    }

    fn pass(&self, ms: i64) {
        self.postmen.postman(IDLE).pass(self.r.at(T0 + ms));
    }

    fn clock(&self) -> i64 {
        self.r.clock.load(Ordering::SeqCst) - T0
    }

    fn state(&self, id: &str) -> Option<(State, Option<Why>)> {
        self.r.row(id).map(|row| (row.state, row.why))
    }

    fn in_queue(&self, text: &str) -> Row {
        let row = self.r.send("job-idle", text);
        self.later(1_000, vec![enqueue(1_000, text)]);
        self.pass(0);
        assert_eq!(self.state(&row.id), Some((State::InQueue, None)));
        self.r.row(&row.id).unwrap()
    }

    fn changed_to(&self, state: &str) -> usize {
        self.r
            .room
            .named("outbox.changed")
            .iter()
            .filter(|c| c["row"]["state"] == state)
            .count()
    }
}

fn before(row: &Row) -> Seen {
    Seen {
        taken: row.seen_before.unwrap_or(0) as usize,
        enqueued: row.queued_before.unwrap_or(0) as usize,
    }
}

#[test]
fn line_written_at_2s_is_sent_before_10s() {
    let e = env();
    let row = e.r.send("job-idle", "hello there");
    e.later(2_000, vec![user(2_000, "hello there")]);
    e.pass(0);
    assert_eq!(e.r.row(&row.id), None);
    assert_eq!(e.clock(), 2_000);
    let gone = e.r.room.named("outbox.removed");
    assert_eq!(gone.last().unwrap()["why"], "sent");
}

#[test]
fn line_written_at_20s_is_sent_at_30s_look() {
    let e = env();
    let row = e.r.send("job-idle", "hello there");
    e.later(20_000, vec![user(20_000, "hello there")]);
    e.pass(0);
    assert_eq!(e.r.row(&row.id), None);
    assert_eq!(e.clock(), 30_000);
}

#[test]
fn clear_is_sent_when_successor_appears() {
    let e = env();
    let row = e.r.send("job-idle", "/clear");
    e.pass(0);
    assert_eq!(e.state(&row.id), Some((State::Waiting, Some(Why::Turn))));
    let next = e.dir.0.join("next.jsonl");
    let head = json!({"type": "system", "agentName": "worker", "content": "SessionStart:clear",
        "timestamp": stamp(5_000)})
    .to_string();
    let write: Write = Box::new(move || append(&next, &[head]));
    e.writes.at.lock().unwrap().push((T0 + 5_000, write));
    e.pass(3_000);
    assert_eq!(e.r.row(&row.id), None);
    assert_eq!(e.clock(), 5_000);
}

#[test]
fn nothing_by_30s_is_unsure() {
    let e = env();
    let row = e.r.send("job-idle", "hello there");
    e.pass(0);
    assert_eq!(e.state(&row.id), Some((State::Unsure, Some(Why::NotFound))));
    assert_eq!(e.clock(), 30_000);
}

#[test]
fn second_identical_row_needs_its_own_line() {
    let e = env();
    let one = e.r.send("job-idle", "ok");
    let two = e.r.send("job-idle", "ok");
    e.later(1_000, vec![user(1_000, "ok")]);
    e.pass(0);
    assert_eq!(e.r.row(&one.id), None);
    assert_eq!(e.state(&two.id), Some((State::Unsure, Some(Why::NotFound))));
}

#[test]
fn enqueue_only_goes_in_queue() {
    let e = env();
    let row = e.in_queue("first");
    assert_eq!(e.clock(), 1_000);
    e.pass(30_000);
    e.pass(60_000);
    assert_eq!(e.state(&row.id), Some((State::InQueue, None)));
    assert_eq!(e.changed_to("in_queue"), 1);
    assert!(e.r.room.named("outbox.removed").is_empty());
}

#[test]
fn in_queue_taken_later_is_sent() {
    for take in [
        vec![operation(120_000, "dequeue"), user(120_000, "first")],
        vec![queued_command(120_000, "first")],
    ] {
        let e = env();
        let row = e.in_queue("first");
        e.pass(60_000);
        e.now(120_000, &take);
        e.pass(120_000);
        assert_eq!(e.r.row(&row.id), None);
        let gone = e.r.room.named("outbox.removed");
        assert_eq!(gone.last().unwrap()["why"], "sent");
    }
}

#[test]
fn in_queue_dropped_by_restart_is_unsure() {
    let e = env();
    let row = e.in_queue("first");
    e.now(60_000, &[user(60_000, "something else")]);
    e.pass(60_000);
    e.pass(89_999);
    assert_eq!(e.state(&row.id), Some((State::InQueue, None)));
    e.pass(90_000);
    assert_eq!(e.state(&row.id), Some((State::Unsure, Some(Why::Dropped))));

    let e = env();
    e.r.files
        .processes
        .lock()
        .unwrap()
        .insert(IDLE.into(), (100, Some(1)));
    let row = e.in_queue("first");
    e.pass(10_000);
    e.r.files
        .processes
        .lock()
        .unwrap()
        .insert(IDLE.into(), (200, Some(2)));
    e.pass(20_000);
    e.pass(49_999);
    assert_eq!(e.state(&row.id), Some((State::InQueue, None)));
    e.pass(50_000);
    assert_eq!(e.state(&row.id), Some((State::Unsure, Some(Why::Dropped))));

    let e = env();
    let row = e.in_queue("first");
    e.now(60_000, &[user(60_000, "something else")]);
    e.pass(60_000);
    e.now(70_000, &[user(70_000, "first")]);
    e.pass(70_000);
    assert_eq!(e.r.row(&row.id), None);
}

#[test]
fn in_queue_popall_without_take_is_unsure() {
    let e = env();
    let row = e.in_queue("first");
    e.now(40_000, &[operation(40_000, "popAll")]);
    e.pass(40_000);
    assert_eq!(e.state(&row.id), Some((State::InQueue, None)));
    e.pass(69_999);
    assert_eq!(e.state(&row.id), Some((State::InQueue, None)));
    e.pass(70_000);
    assert_eq!(e.state(&row.id), Some((State::Unsure, Some(Why::Dropped))));
}

#[test]
fn no_line_within_5s_turns_safe_mode_on() {
    let e = env();
    let row = e.r.send("job-idle", "first");
    e.later(7_000, vec![user(7_000, "first")]);
    e.pass(0);
    assert_eq!(e.r.row(&row.id), None);
    assert!(e.r.safe.is_safe(IDLE, &QueueState::default()));
}

#[test]
fn a_later_line_within_5s_turns_it_off() {
    let e = env();
    e.r.send("job-idle", "first");
    e.later(7_000, vec![user(7_000, "first")]);
    e.pass(0);
    assert!(e.r.safe.is_safe(IDLE, &QueueState::default()));
    let row = e.r.send("job-idle", "second");
    e.pass(7_000);
    assert_eq!(e.state(&row.id), Some((State::Waiting, Some(Why::Turn))));
    e.later(11_000, vec![enqueue(11_000, "second")]);
    e.pass(10_000);
    assert_eq!(e.state(&row.id), Some((State::InQueue, None)));
    assert!(!e.r.safe.is_safe(IDLE, &QueueState::default()));
}

#[test]
fn unknown_queue_line_makes_only_that_session_safe() {
    let safe = SafeModes::default();
    let odd = QueueState {
        waiting: Vec::new(),
        unknown: 1,
    };
    assert!(safe.is_safe("one", &odd));
    assert!(!safe.is_safe("two", &QueueState::default()));
    safe.note("one", false);
    assert!(safe.is_safe("one", &QueueState::default()));
    assert!(!safe.is_safe("two", &QueueState::default()));
    safe.note("one", true);
    assert!(!safe.is_safe("one", &QueueState::default()));
}

#[test]
fn verifier_and_courier_share_safe_modes() {
    let e = env();
    e.r.send("job-idle", "first");
    e.later(7_000, vec![user(7_000, "first")]);
    e.pass(0);
    let row = e.r.send("job-idle", "second");
    assert!(e.r.courier.facts(&row, T0 + 8_000).safe);
}

#[test]
fn in_queue_row_at_start_keeps_watching() {
    let e = env();
    let row = e.in_queue("first");
    e.r.ops.load(T0 + 2_000);
    assert_eq!(e.state(&row.id), Some((State::InQueue, None)));
    e.now(
        40_000,
        &[operation(40_000, "dequeue"), user(40_000, "first")],
    );
    e.pass(40_000);
    assert_eq!(e.r.row(&row.id), None);
}

#[test]
fn dropped_verdict_runs_at_30s_without_file_change() {
    let e = env();
    let row = e.in_queue("first");
    e.now(40_000, &[remove(40_000, "first")]);
    e.pass(40_000);
    assert_eq!(e.verifier.next_look_ms(&row), Some(T0 + 70_000));
    let base = before(&row);
    assert_eq!(
        e.verifier.watch(&row, base, T0 + 69_999),
        QueuedVerdict::Stay
    );
    assert_eq!(
        e.verifier.watch(&row, base, T0 + 70_000),
        QueuedVerdict::Unsure
    );
}

#[test]
fn listed_entry_in_a_stopped_or_restarted_job_is_not_listed() {
    let e = env();
    let row = e.in_queue("first");
    let base = before(&row);
    e.r.files.set_state(IDLE, "stopped");
    assert_eq!(
        e.verifier.watch(&row, base, T0 + 10_000),
        QueuedVerdict::Stay
    );
    assert_eq!(e.verifier.next_look_ms(&row), Some(T0 + 40_000));
    assert_eq!(
        e.verifier.watch(&row, base, T0 + 40_000),
        QueuedVerdict::Unsure
    );

    let e = env();
    e.r.files
        .processes
        .lock()
        .unwrap()
        .insert(IDLE.into(), (100, None));
    let row = e.in_queue("first");
    let base = before(&row);
    e.r.files
        .processes
        .lock()
        .unwrap()
        .insert(IDLE.into(), (200, None));
    assert_eq!(
        e.verifier.watch(&row, base, T0 + 10_000),
        QueuedVerdict::Stay
    );
    assert_eq!(
        e.verifier.watch(&row, base, T0 + 20_000),
        QueuedVerdict::Stay
    );
    assert_eq!(e.verifier.next_look_ms(&row), Some(T0 + 40_000));
}

#[test]
fn vanished_job_in_queue_waits_60s_then_30s() {
    let e = env();
    let row = e.in_queue("first");
    let base = before(&row);
    assert_eq!(
        e.verifier.watch(&row, base, T0 + 2_000),
        QueuedVerdict::Stay
    );
    let agent = e.r.files.vanish(IDLE).unwrap();
    let at = |ms: i64| e.verifier.watch(&row, base, T0 + 10_000 + ms);
    assert_eq!(at(0), QueuedVerdict::Stay);
    assert_eq!(at(60_000), QueuedVerdict::Stay);
    assert_eq!(e.verifier.next_look_ms(&row), Some(T0 + 100_000));
    assert_eq!(at(89_999), QueuedVerdict::Stay);
    assert_eq!(at(90_000), QueuedVerdict::Unsure);

    let e = env();
    let row = e.in_queue("first");
    let base = before(&row);
    let at = |ms: i64| e.verifier.watch(&row, base, T0 + 10_000 + ms);
    assert_eq!(at(0), QueuedVerdict::Stay);
    e.r.files.vanish(IDLE);
    assert_eq!(at(1), QueuedVerdict::Stay);
    assert_eq!(at(50_000), QueuedVerdict::Stay);
    e.r.files.put(IDLE, agent);
    assert_eq!(at(50_001), QueuedVerdict::Stay);
    assert_eq!(at(200_000), QueuedVerdict::Stay);
    assert_eq!(e.verifier.next_look_ms(&row), None);
}

fn set_process(e: &Env, process: Option<(u32, Option<i64>)>) {
    let mut all = e.r.files.processes.lock().unwrap();
    match process {
        Some(p) => all.insert(IDLE.into(), p),
        None => all.remove(IDLE),
    };
}

#[test]
fn a_watch_is_not_evicted_by_age() {
    let e = env();
    set_process(&e, Some((100, None)));
    let row = e.in_queue("first");
    let base = before(&row);
    assert_eq!(
        e.verifier.watch(&row, base, T0 + 2_000),
        QueuedVerdict::Stay
    );
    let other = e.r.send("job-idle2", "elsewhere");
    assert_eq!(
        e.verifier.watch(&other, before(&other), T0 + 700_000),
        QueuedVerdict::Stay
    );
    set_process(&e, Some((200, None)));
    e.verifier.watch(&row, base, T0 + 700_001);
    assert_eq!(e.verifier.next_look_ms(&row), Some(T0 + 730_001));
}

#[test]
fn a_row_that_ends_drops_its_watch() {
    let e = env();
    let row = e.in_queue("first");
    e.verifier.watch(&row, before(&row), T0 + 2_000);
    assert_eq!(e.verifier.watching(), 1);
    e.r.ops.store().delete(&row.id).unwrap();
    e.r.ops.publish_removed(&row.id, &row.session, "discarded");
    assert_eq!(e.verifier.watching(), 0);

    let row = e.in_queue("second");
    e.verifier.watch(&row, before(&row), T0 + 3_000);
    assert_eq!(e.verifier.watching(), 1);
    e.verifier.forget(&row.id);
    assert_eq!(e.verifier.watching(), 0);
}

#[test]
fn a_changed_process_stays_changed_when_it_reads_as_none() {
    let e = env();
    set_process(&e, Some((100, None)));
    let row = e.in_queue("first");
    let base = before(&row);
    set_process(&e, Some((200, None)));
    assert_eq!(
        e.verifier.watch(&row, base, T0 + 10_000),
        QueuedVerdict::Stay
    );
    set_process(&e, None);
    assert_eq!(
        e.verifier.watch(&row, base, T0 + 20_000),
        QueuedVerdict::Stay
    );
    assert_eq!(e.verifier.next_look_ms(&row), Some(T0 + 40_000));
    assert_eq!(
        e.verifier.watch(&row, base, T0 + 40_000),
        QueuedVerdict::Unsure
    );
}

#[test]
fn the_first_watch_of_a_vanished_job_still_waits_60s_then_30s() {
    let e = env();
    let row = e.in_queue("first");
    let base = before(&row);
    e.r.files.vanish(IDLE);
    let at = |ms: i64| e.verifier.watch(&row, base, T0 + 10_000 + ms);
    assert_eq!(at(0), QueuedVerdict::Stay);
    assert_eq!(at(30_000), QueuedVerdict::Stay);
    assert_eq!(at(60_000), QueuedVerdict::Stay);
    assert_eq!(at(89_999), QueuedVerdict::Stay);
    assert_eq!(at(90_000), QueuedVerdict::Unsure);
}

#[test]
fn any_new_jsonl_in_the_folder_wakes_a_clear_waiter() {
    let file = Path::new("/w/projects/demo/old.jsonl");
    let batch = |name: &str| Batch {
        paths: [PathBuf::from("/w/projects/demo").join(name)].into(),
        rescan: false,
    };
    assert!(names(&batch("old.jsonl"), file, false));
    assert!(!names(&batch("next.jsonl"), file, false));
    assert!(names(&batch("next.jsonl"), file, true));
    assert!(!names(&batch("notes.txt"), file, true));
}

#[test]
fn a_real_clear_waiter_wakes_on_the_successor_file() {
    let dir = Folder::new("real-wait");
    let old = dir.0.join("old.jsonl");
    append(&old, &[operation(0, "dequeue")]);
    let clock = real_clock();
    let pause = RealPause {
        clock: clock.clone(),
        closed: Arc::new(AtomicBool::new(false)),
    };
    let mut waits = pause.watch(&old, true);
    let next = dir.0.join("next.jsonl");
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        append(&next, &[operation(0, "dequeue")]);
    });
    let started = clock();
    waits.next(started + 20_000);
    writer.join().unwrap();
    assert!(clock() - started < 10_000);
}

#[test]
fn merged_line_makes_in_queue_row_sent() {
    let e = env();
    let row = e.in_queue("first");
    e.now(
        60_000,
        &[
            enqueue(50_000, "mine"),
            operation(60_000, "dequeue"),
            operation(60_000, "dequeue"),
            user(60_000, "first\nmine"),
        ],
    );
    e.pass(60_000);
    assert_eq!(e.r.row(&row.id), None);
}

struct Start {
    r: Rig,
    dir: Folder,
    ops: OutboxOps,
    transcript: PathBuf,
}

const JOB: &str = "be50d36a";

fn start() -> Start {
    let r = rig();
    let dir = Folder::new("start");
    let transcript = dir.0.join("projects/demo/fee7356b.jsonl");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    append(&transcript, &[operation(-60_000, "dequeue")]);
    let job = json!({"state": "working", "tempo": "idle", "detail": "x", "template": "bg",
        "name": "main", "sessionId": "fee7356b-b891-4f6f-b4e7-35f7093b251c",
        "linkScanPath": transcript.to_string_lossy(), "cwd": "/work/demo",
        "updatedAt": "2026-11-14T22:54:49.559Z"});
    let state = dir.0.join(format!("claude/jobs/{JOB}/state.json"));
    std::fs::create_dir_all(state.parent().unwrap()).unwrap();
    std::fs::write(&state, job.to_string()).unwrap();
    let roots = Roots {
        main_dir: dir.0.join("claude"),
        lanes_file: dir.0.join("none/lanes.json"),
    };
    let ops = OutboxOps::new(
        Arc::new(OutboxStore::in_memory_for_tests()),
        Arc::new(Mutex::new(json!([]))),
        roots,
        r.acting_switch(),
        r.room.clone(),
        None,
    );
    Start {
        r,
        dir,
        ops,
        transcript,
    }
}

impl Start {
    fn sending(&self, session: &str, tool: Tool, text: &str, seen: Option<Seen>) -> Row {
        let store = self.ops.store();
        let new = New {
            session: session.into(),
            tool,
            text: text.into(),
            options: None,
            via: Via::Chat,
            clear_box: false,
        };
        let row = store.insert(&new, T0 - 1_000).unwrap();
        store.take(&row.id, T0).unwrap().unwrap();
        if let Some(seen) = seen {
            store.set_seen(&row.id, seen).unwrap();
        }
        row
    }

    fn state(&self, id: &str) -> Option<(State, Option<Why>)> {
        self.ops
            .store()
            .get(id)
            .unwrap()
            .map(|row| (row.state, row.why))
    }
}

#[test]
fn sending_row_found_at_start_is_deleted() {
    let s = start();
    append(&s.transcript, &[user(-10_000, "hi"), user(1_000, "hi")]);
    let row = s.sending(
        JOB,
        Tool::Claude,
        "hi",
        Some(Seen {
            enqueued: 0,
            taken: 0,
        }),
    );
    s.ops.load(T0 + 60_000);
    assert_eq!(s.state(&row.id), None);
    let gone = s.r.room.named("outbox.removed");
    assert_eq!(gone.last().unwrap()["why"], "sent");
}

#[test]
fn sending_row_enqueued_only_at_start_is_in_queue() {
    let s = start();
    append(&s.transcript, &[enqueue(1_000, "hi")]);
    let row = s.sending(JOB, Tool::Claude, "hi", Some(Seen::default()));
    s.ops.load(T0 + 60_000);
    assert_eq!(s.state(&row.id), Some((State::InQueue, None)));
}

#[test]
fn sending_row_not_found_at_start_is_unsure() {
    let s = start();
    append(&s.transcript, &[user(1_000, "something else")]);
    let row = s.sending(JOB, Tool::Claude, "hi", Some(Seen::default()));
    s.ops.load(T0 + 60_000);
    assert_eq!(s.state(&row.id), Some((State::Unsure, Some(Why::Restart))));
}

#[test]
fn sending_row_without_seen_before_is_unsure() {
    let s = start();
    append(&s.transcript, &[user(1_000, "hi")]);
    let row = s.sending(JOB, Tool::Claude, "hi", None);
    s.ops.load(T0 + 60_000);
    assert_eq!(s.state(&row.id), Some((State::Unsure, Some(Why::Restart))));
}

#[test]
fn codex_sending_row_at_start_is_unsure() {
    let s = start();
    let row = s.sending("codex:t1", Tool::Codex, "hi", Some(Seen::default()));
    s.ops.load(T0 + 60_000);
    assert_eq!(s.state(&row.id), Some((State::Unsure, Some(Why::Restart))));
    assert!(s.dir.0.is_dir());
}

#[test]
fn a_closed_verifier_stops_waiting_and_leaves_the_verdict_open() {
    let roots = Roots {
        main_dir: PathBuf::from("/nonexistent/claude"),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let verifier = TranscriptVerifier::new(roots, Arc::default());
    verifier.close();
    let r = rig();
    let row = r.send("job-idle", "hi");
    let entered = crate::acting_ops::now_ms();
    assert_eq!(
        verifier.wait(&row, Seen::default(), entered),
        Verdict::Pending
    );
}
