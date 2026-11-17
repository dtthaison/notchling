use super::*;
use crate::event_assert::{of, text};
use crate::outbox_courier::Courier;
use crate::outbox_postman::{Baseline, Couriers, Next, Postmen};
use crate::outbox_testkit::{rig, Rig, IDLE, T0};
use notchling_engine::bridge::net::files::iso8601;
use notchling_engine::outbox::{Kind, State, Tool, Via, Why};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

static FOLDERS: AtomicUsize = AtomicUsize::new(0);

struct Folder(PathBuf);

impl Folder {
    fn new(tag: &str) -> Folder {
        let n = FOLDERS.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("nl-outbox-late-{tag}-{}-{n}", std::process::id()));
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

type Step = Box<dyn FnOnce() + Send>;

struct Skip {
    clock: Arc<AtomicI64>,
    steps: Mutex<Vec<(i64, Step)>>,
    watched: AtomicUsize,
}

impl Skip {
    fn to(&self, deadline: i64) {
        loop {
            let next = {
                let mut steps = self.steps.lock().unwrap();
                let soonest = steps
                    .iter()
                    .enumerate()
                    .filter(|(_, (at, _))| *at <= deadline)
                    .min_by_key(|(_, (at, _))| *at)
                    .map(|(n, _)| n);
                soonest.map(|n| steps.remove(n))
            };
            match next {
                Some((at, step)) => {
                    self.clock.fetch_max(at, Ordering::SeqCst);
                    step();
                }
                None => {
                    self.clock.fetch_max(deadline, Ordering::SeqCst);
                    return;
                }
            }
        }
    }
}

struct Jump<'a>(&'a Skip);

impl Waits for Jump<'_> {
    fn next(&mut self, deadline_ms: i64) {
        self.0.to(deadline_ms);
    }
}

impl Pause for Skip {
    fn watch(&self, _file: &Path, _any_jsonl: bool) -> Box<dyn Waits + '_> {
        self.watched.fetch_add(1, Ordering::SeqCst);
        Box::new(Jump(self))
    }

    fn sleep(&self, deadline_ms: i64) {
        self.to(deadline_ms);
    }
}

struct Env {
    r: Rig,
    _dir: Folder,
    path: PathBuf,
    skip: Arc<Skip>,
    postmen: Postmen,
}

fn stamp(ms: i64) -> String {
    iso8601(T0 + ms)
}

fn user(ms: i64, text: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": text}, "uuid": "u",
        "timestamp": stamp(ms), "sessionId": "s-1", "cwd": "/work/demo"})
    .to_string()
}

fn other(ms: i64) -> String {
    json!({"type": "user", "message": {"role": "user", "content": "something else"}, "uuid": "o",
        "timestamp": stamp(ms), "sessionId": "s-1", "cwd": "/work/demo"})
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

struct Racing {
    inner: Arc<TranscriptVerifier>,
    during: Arc<Mutex<Option<Step>>>,
}

impl Verifier for Racing {
    fn seen(&self, row: &Row) -> Baseline {
        self.inner.seen(row)
    }

    fn wait(&self, row: &Row, before: Seen, entered_ms: i64) -> Verdict {
        self.inner.wait(row, before, entered_ms)
    }

    fn watch(&self, row: &Row, before: Seen, now_ms: i64) -> QueuedVerdict {
        self.inner.watch(row, before, now_ms)
    }

    fn next_look_ms(&self, row: &Row) -> Option<i64> {
        self.inner.next_look_ms(row)
    }

    fn looks_again(&self) -> bool {
        true
    }

    fn found_late(&self, row: &Row, before: Seen) -> bool {
        let found = self.inner.found_late(row, before);
        if let Some(step) = self.during.lock().unwrap().take() {
            step();
        }
        found
    }

    fn forget(&self, id: &str) {
        self.inner.forget(id);
    }
}

fn env(linked: bool) -> Env {
    build(linked, |inner| inner)
}

fn build(linked: bool, wrap: impl FnOnce(Arc<TranscriptVerifier>) -> Arc<dyn Verifier>) -> Env {
    let r = rig();
    let dir = Folder::new("env");
    let path = dir.0.join("idle.jsonl");
    append(&path, &[other(-60_000)]);
    let mut agent = r.files.row(IDLE).unwrap();
    agent.transcript_path = linked.then(|| path.to_string_lossy().into_owned());
    agent.name = "worker".into();
    r.files.put(IDLE, agent);
    let ticks = r.clock.clone();
    let clock: Clock = Arc::new(move || ticks.load(Ordering::SeqCst));
    let skip = Arc::new(Skip {
        clock: r.clock.clone(),
        steps: Mutex::new(Vec::new()),
        watched: AtomicUsize::new(0),
    });
    let verifier = wrap(Arc::new(TranscriptVerifier::with_parts(
        r.files.clone(),
        clock.clone(),
        skip.clone(),
        r.safe.clone(),
    )));
    let postmen = Postmen::build(
        r.ops.clone(),
        Couriers::claude_only(r.courier.clone() as Arc<dyn Courier>),
        verifier,
        r.acting_switch(),
        clock,
    );
    Env {
        r,
        _dir: dir,
        path,
        skip,
        postmen,
    }
}

impl Env {
    fn at(&self, ms: i64, step: impl FnOnce() + Send + 'static) {
        self.skip
            .steps
            .lock()
            .unwrap()
            .push((T0 + ms, Box::new(step)));
    }

    fn link_at(&self, ms: i64) {
        let files = self.r.files.clone();
        let path = self.path.to_string_lossy().into_owned();
        self.at(ms, move || {
            let mut agent = files.row(IDLE).unwrap();
            agent.transcript_path = Some(path);
            files.put(IDLE, agent);
        });
    }

    fn state(&self, id: &str) -> Option<(State, Option<Why>)> {
        self.r.row(id).map(|row| (row.state, row.why))
    }

    fn unsure(&self, text: &str) -> (String, crate::outbox_postman::Postman) {
        let row = self.r.send("job-idle", text);
        let mut postman = self.postmen.postman(IDLE);
        postman.pass(self.r.at(T0));
        assert_eq!(
            self.state(&row.id),
            Some((State::Unsure, Some(Why::NotFound)))
        );
        assert_eq!(self.r.clock.load(Ordering::SeqCst) - T0, 30_000);
        (row.id, postman)
    }

    fn told(&self) -> Vec<String> {
        of(&self.r.sink, "session.outbox")
            .iter()
            .filter_map(|e| text(e, "then"))
            .collect()
    }
}

#[test]
fn a_transcript_that_is_linked_late_is_still_read_inside_the_look() {
    let e = env(false);
    let row = e.r.send("job-idle", "hello there");
    append(&e.path, &[user(2_000, "hello there")]);
    e.link_at(3_000);
    e.postmen.postman(IDLE).pass(e.r.at(T0));
    assert_eq!(e.state(&row.id), None, "the line was there from 2 s");
    let gone = e.r.room.named("outbox.removed");
    assert_eq!(gone.last().unwrap()["why"], "sent");
    assert_eq!(
        e.skip.watched.load(Ordering::SeqCst),
        1,
        "the file is watched from the moment it is found"
    );
}

#[test]
fn a_job_with_no_link_path_yet_is_found_by_its_session_id() {
    let dir = Folder::new("live");
    let roots = Roots {
        main_dir: dir.0.join("claude"),
        lanes_file: dir.0.join("lanes.json"),
    };
    let job = roots.main_dir.join("jobs").join("job-x");
    std::fs::create_dir_all(&job).unwrap();
    std::fs::write(
        job.join("state.json"),
        json!({"state": "working", "sessionId": "sess-1", "name": "worker",
            "cwd": "/work/demo", "updatedAt": "2026-11-17T03:49:31.000Z"})
        .to_string(),
    )
    .unwrap();
    let project = roots.main_dir.join("projects").join("-work-demo");
    std::fs::create_dir_all(&project).unwrap();
    append(&project.join("sess-1.jsonl"), &[user(2_000, "hello there")]);
    let verifier = TranscriptVerifier::new(roots, Arc::new(SafeModes::default()));
    let row = Row {
        n: 1,
        id: "o1".into(),
        session: "job-x".into(),
        tool: Tool::Claude,
        kind: Kind::Prompt,
        text: "hello there".into(),
        options: None,
        via: Via::Chat,
        state: State::Sending,
        why: None,
        clear_box: false,
        queued_ms: T0,
        updated_ms: T0,
        attempt_ms: Some(T0),
        seen_before: Some(0),
        queued_before: Some(0),
        tries: 1,
        next_ms: None,
        noticed_ms: None,
        anchor: None,
    };
    let based = verifier.seen(&row);
    assert_eq!(based.seen.taken, 1, "{based:?}");
}

#[test]
fn an_unsure_message_the_transcript_later_holds_is_sent_not_left_to_send_again() {
    let e = env(true);
    let (id, mut postman) = e.unsure("hello there");
    assert_eq!(e.r.returns(), 1);
    append(&e.path, &[user(40_000, "hello there")]);
    postman.pass(e.r.at(T0 + 31_000));
    assert_eq!(
        e.state(&id),
        Some((State::Unsure, Some(Why::NotFound))),
        "not looked at again before the next look"
    );
    postman.pass(e.r.at(T0 + 45_000));
    assert_eq!(e.state(&id), None);
    let gone = e.r.room.named("outbox.removed");
    assert_eq!(gone.last().unwrap()["why"], "sent");
    let told = e.told();
    assert_eq!(told.last().map(String::as_str), Some("sent"));
    assert!(told.iter().any(|then| then == "unsure"), "{told:?}");
    assert_eq!(e.r.returns(), 1, "never typed again");
}

#[test]
fn an_unsure_message_stays_when_the_transcript_only_has_an_older_copy() {
    let e = env(true);
    append(&e.path, &[user(-30_000, "hello there")]);
    let (id, mut postman) = e.unsure("hello there");
    postman.pass(e.r.at(T0 + 45_000));
    postman.pass(e.r.at(T0 + 90_000));
    assert_eq!(e.state(&id), Some((State::Unsure, Some(Why::NotFound))));
    assert!(e.r.room.named("outbox.removed").is_empty());
}

#[test]
fn an_unsure_message_is_looked_for_a_while_and_a_new_postman_looks_once() {
    let e = env(true);
    let (id, mut postman) = e.unsure("hello there");
    postman.pass(e.r.at(T0 + 120_000));
    assert!(matches!(postman.pass(e.r.at(T0 + 700_000)), Next::Done));
    append(&e.path, &[user(710_000, "hello there")]);
    assert!(matches!(postman.pass(e.r.at(T0 + 720_000)), Next::Done));
    assert_eq!(e.state(&id), Some((State::Unsure, Some(Why::NotFound))));
    let mut fresh = e.postmen.postman(IDLE);
    fresh.pass(e.r.at(T0 + 730_000));
    assert_eq!(e.state(&id), None, "a restarted daemon looks once");
    assert_eq!(e.r.returns(), 1);
}

#[test]
fn an_unsure_message_with_no_baseline_is_never_taken_for_sent() {
    let e = env(true);
    let (id, mut postman) = e.unsure("hello there");
    e.r.ops.store().run_sql_for_tests(&format!(
        "UPDATE outbox SET seen_before = NULL, queued_before = NULL WHERE id = '{id}'"
    ));
    append(&e.path, &[user(40_000, "hello there")]);
    postman.pass(e.r.at(T0 + 45_000));
    postman.pass(e.r.at(T0 + 90_000));
    assert_eq!(e.state(&id), Some((State::Unsure, Some(Why::NotFound))));
    assert!(e.r.room.named("outbox.removed").is_empty());
}

#[test]
fn a_row_sent_again_while_the_look_runs_is_not_deleted_as_sent() {
    let during: Arc<Mutex<Option<Step>>> = Arc::default();
    let hook = during.clone();
    let e = build(true, move |inner| {
        Arc::new(Racing {
            inner,
            during: hook,
        })
    });
    let (id, mut postman) = e.unsure("hello there");
    append(&e.path, &[user(40_000, "hello there")]);
    let ops = e.r.ops.clone();
    let again = id.clone();
    *during.lock().unwrap() = Some(Box::new(move || {
        let moved = ops
            .store()
            .set_state(
                &again,
                &[State::Unsure],
                State::Queued,
                None,
                None,
                T0 + 40_000,
            )
            .unwrap();
        assert!(moved);
    }));
    postman.pass(e.r.at(T0 + 45_000));
    let (state, _) = e
        .state(&id)
        .expect("the row the person sent again is still there");
    assert!(matches!(state, State::Queued | State::Waiting), "{state:?}");
    assert!(e.r.room.named("outbox.removed").is_empty());
    assert_ne!(e.told().last().map(String::as_str), Some("sent"));
}

#[test]
fn a_second_unsure_spell_waits_its_own_look_interval() {
    let e = env(true);
    let (id, mut postman) = e.unsure("hello there");
    postman.pass(e.r.at(T0 + 45_000));
    assert_eq!(e.state(&id), Some((State::Unsure, Some(Why::NotFound))));
    e.r.ops.store().run_sql_for_tests(&format!(
        "UPDATE outbox SET state = 'unsure', why = 'not_found', updated_ms = {} WHERE id = '{id}'",
        T0 + 60_000
    ));
    append(&e.path, &[user(61_000, "hello there")]);
    postman.pass(e.r.at(T0 + 62_000));
    assert_eq!(
        e.state(&id),
        Some((State::Unsure, Some(Why::NotFound))),
        "not before its own interval"
    );
    postman.pass(e.r.at(T0 + 66_000));
    assert_eq!(e.state(&id), None);
}

#[test]
fn an_unsure_row_tells_the_app_when_its_attempt_began() {
    let e = env(true);
    let (id, _postman) = e.unsure("hello there");
    let unsure = e.r.row(&id).unwrap();
    let wire = crate::outbox_rows::wire(&unsure, 1, None);
    assert_eq!(wire["attemptMs"], json!(T0));
    assert_eq!(wire["updatedMs"], json!(T0 + 30_000));
    let queued = e.r.send("job-idle", "and then this");
    let behind = crate::outbox_rows::wire(&queued, 2, None);
    assert!(behind.get("attemptMs").is_none());
}

#[test]
fn a_row_dropped_while_the_look_runs_is_not_announced_as_sent() {
    let during: Arc<Mutex<Option<Step>>> = Arc::default();
    let hook = during.clone();
    let e = build(true, move |inner| {
        Arc::new(Racing {
            inner,
            during: hook,
        })
    });
    let (id, mut postman) = e.unsure("hello there");
    append(&e.path, &[user(40_000, "hello there")]);
    let ops = e.r.ops.clone();
    let dropped = id.clone();
    *during.lock().unwrap() = Some(Box::new(move || {
        ops.store().delete(&dropped).unwrap();
    }));
    postman.pass(e.r.at(T0 + 45_000));
    assert_eq!(e.state(&id), None);
    assert!(e.r.room.named("outbox.removed").is_empty());
    assert_ne!(e.told().last().map(String::as_str), Some("sent"));
}
