use super::*;
use crate::outbox_courier::Courier;
use crate::outbox_ops::OutboxOps;
use crate::outbox_postman::{Couriers, Postmen};
use crate::outbox_testkit::{rig, Rig, IDLE, T0};
use notchling_engine::bridge::net::files::iso8601;
use notchling_engine::outbox::{New, OutboxStore, State, Tool, Via, Why, UNSURE_HOLD_MS};
use notchling_engine::sessions::Roots;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

static FOLDERS: AtomicUsize = AtomicUsize::new(0);

const ZWSP: char = '\u{200b}';
const WORDS: [&str; 8] = [
    "carry", "the", "blue", "lantern", "across", "the", "quiet", "bridge",
];

struct Folder(PathBuf);

impl Folder {
    fn new() -> Folder {
        let n = FOLDERS.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("nl-outbox-invisible-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("drops one")).unwrap();
        Folder(dir)
    }

    fn shot(&self) -> PathBuf {
        let path = self.0.join("drops one").join("Shot 1.png");
        std::fs::write(&path, b"png").unwrap();
        path
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
        Box::new(Jump(self))
    }

    fn sleep(&self, deadline_ms: i64) {
        self.to(deadline_ms);
    }
}

struct Env {
    r: Rig,
    dir: Folder,
    path: PathBuf,
    skip: Arc<Skip>,
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

fn dequeue(ms: i64) -> String {
    json!({"type": "queue-operation", "operation": "dequeue", "timestamp": stamp(ms),
        "sessionId": "s-1"})
    .to_string()
}

fn turn(ms: i64, text: &str, images: usize) -> String {
    let mut blocks = vec![json!({"type": "text", "text": text})];
    for _ in 0..images {
        blocks.push(json!({"type": "image", "source": {"type": "base64", "data": "AAAA"}}));
    }
    json!({"type": "user", "uuid": "u", "timestamp": stamp(ms), "sessionId": "s-1",
        "cwd": "/work/demo", "origin": {"kind": "human"}, "imagePasteIds": [2],
        "message": {"role": "user", "content": blocks}})
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

fn env() -> Env {
    let r = rig();
    let dir = Folder::new();
    let path = dir.0.join("idle.jsonl");
    append(&path, &[other(-60_000)]);
    let mut agent = r.files.row(IDLE).unwrap();
    agent.transcript_path = Some(path.to_string_lossy().into_owned());
    agent.name = "worker".into();
    r.files.put(IDLE, agent);
    let ticks = r.clock.clone();
    let clock: Clock = Arc::new(move || ticks.load(Ordering::SeqCst));
    let skip = Arc::new(Skip {
        clock: r.clock.clone(),
        steps: Mutex::new(Vec::new()),
    });
    let verifier = Arc::new(TranscriptVerifier::with_parts(
        r.files.clone(),
        clock.clone(),
        skip.clone(),
        r.safe.clone(),
    ));
    let postmen = Postmen::build(
        r.ops.clone(),
        Couriers::claude_only(r.courier.clone() as Arc<dyn Courier>),
        verifier,
        r.acting_switch(),
        clock,
    );
    Env {
        r,
        dir,
        path,
        skip,
        postmen,
    }
}

impl Env {
    fn picture_message(&self) -> String {
        let sentence = WORDS.join(&ZWSP.to_string());
        format!(
            "why this lamp \n{sentence}\n\n'{}'",
            self.dir.shot().display()
        )
    }

    fn stored(&self) -> String {
        format!("[Image #2]why this lamp \n{}", WORDS.concat())
    }

    fn the_turn(&self, stored: &str, at_ms: i64) -> Vec<String> {
        vec![
            enqueue(at_ms - 1_000, stored),
            dequeue(at_ms),
            turn(at_ms, stored, 1),
        ]
    }

    fn turn_at(&self, ms: i64, stored: &str) {
        let lines = self.the_turn(stored, ms);
        let path = self.path.clone();
        self.skip
            .steps
            .lock()
            .unwrap()
            .push((T0 + ms, Box::new(move || append(&path, &lines))));
    }

    fn state(&self, id: &str) -> Option<(State, Option<Why>)> {
        self.r.row(id).map(|row| (row.state, row.why))
    }
}

#[cfg(unix)]
#[test]
fn a_picture_message_with_zero_width_spaces_is_sent_when_the_transcript_shows_its_turn() {
    let e = env();
    let row = e.r.send("job-idle", &e.picture_message());
    e.turn_at(3_000, &e.stored());
    e.postmen.postman(IDLE).pass(e.r.at(T0));
    assert_eq!(e.state(&row.id), None, "Claude Code took it at 3 s");
    let gone = e.r.room.named("outbox.removed");
    assert_eq!(gone.last().unwrap()["why"], "sent");
    assert_eq!(e.r.returns(), 1);
}

#[cfg(unix)]
#[test]
fn an_unsure_picture_message_with_zero_width_spaces_is_sent_when_its_turn_appears_later() {
    let e = env();
    let row = e.r.send("job-idle", &e.picture_message());
    let mut postman = e.postmen.postman(IDLE);
    postman.pass(e.r.at(T0));
    assert_eq!(
        e.state(&row.id),
        Some((State::Unsure, Some(Why::NotFound))),
        "nothing in the transcript within 30 s"
    );
    append(&e.path, &e.the_turn(&e.stored(), 40_000));
    postman.pass(e.r.at(T0 + 45_000));
    assert_eq!(e.state(&row.id), None, "the late look finds the turn");
    let gone = e.r.room.named("outbox.removed");
    assert_eq!(gone.last().unwrap()["why"], "sent");
    assert_eq!(e.r.returns(), 1, "never typed again");
}

#[cfg(unix)]
#[test]
fn a_late_proof_settles_a_row_that_stepped_aside_and_it_is_never_typed_again() {
    let e = env();
    let row = e.r.send("job-idle", &e.picture_message());
    let mut postman = e.postmen.postman(IDLE);
    postman.pass(e.r.at(T0));
    assert_eq!(e.state(&row.id), Some((State::Unsure, Some(Why::NotFound))));
    let aside_at = T0 + 2 * UNSURE_HOLD_MS;
    postman.pass(e.r.at(aside_at));
    assert!(e
        .r
        .row(&row.id)
        .is_some_and(|row| row.aside_ms.is_some() && row.state == State::Unsure));
    append(
        &e.path,
        &e.the_turn(&e.stored(), 2 * UNSURE_HOLD_MS + 1_000),
    );
    postman.pass(e.r.at(aside_at + 10_000));
    assert_eq!(e.state(&row.id), None, "the late look finds the turn");
    let gone = e.r.room.named("outbox.removed");
    assert_eq!(gone.last().unwrap()["why"], "sent");
    assert_eq!(e.r.returns(), 1, "never typed again");
}

#[cfg(unix)]
#[test]
fn the_next_message_goes_once_the_one_before_it_is_found() {
    let e = env();
    let first = e.r.send("job-idle", &e.picture_message());
    let second = e.r.send_with("job-idle", "and then the next one", true);
    e.turn_at(3_000, &e.stored());
    e.postmen.postman(IDLE).pass(e.r.at(T0));
    assert_eq!(e.state(&first.id), None);
    assert_eq!(e.r.returns(), 2, "the second was typed behind the first");
    assert!(e.r.typed().contains("and then the next one"));
    assert!(e
        .r
        .row(&second.id)
        .is_some_and(|r| r.state != State::Queued));
}

#[test]
fn a_turn_with_other_visible_words_leaves_the_row_unsure_and_holds_the_next_one_for_two_minutes() {
    let e = env();
    let first = e.r.send("job-idle", &e.picture_message());
    let second = e.r.send_with("job-idle", "and then the next one", true);
    let other_words = format!("[Image #2]why this lamp \n{}", "carry the blue lamp");
    e.turn_at(3_000, &other_words);
    let mut postman = e.postmen.postman(IDLE);
    postman.pass(e.r.at(T0));
    postman.pass(e.r.at(T0 + 45_000));
    assert_eq!(
        e.state(&first.id),
        Some((State::Unsure, Some(Why::NotFound)))
    );
    assert_eq!(e.state(&second.id), Some((State::Queued, None)));
    assert_eq!(
        e.r.returns(),
        1,
        "an unsure head holds the session's mailbox"
    );
    postman.pass(e.r.at(T0 + 2 * UNSURE_HOLD_MS));
    assert!(e
        .r
        .row(&first.id)
        .is_some_and(|row| row.state == State::Unsure && row.aside_ms.is_some()));
    assert_eq!(
        e.state(&second.id),
        Some((State::Waiting, Some(Why::Turn))),
        "the head stepped aside, so the next is judged by its own gate"
    );
}

fn absorbed(ms: i64, text: &str) -> Vec<String> {
    vec![
        enqueue(ms, text),
        json!({"type": "queue-operation", "operation": "remove", "timestamp": stamp(ms + 2_400),
            "sessionId": "s-1", "content": text})
        .to_string(),
        json!({"type": "attachment", "timestamp": stamp(ms - 40), "sessionId": "s-1",
            "attachment": {"type": "queued_command", "commandMode": "prompt",
                "origin": {"kind": "human"}, "imagePasteIds": [1],
                "prompt": [{"type": "text", "text": text},
                    {"type": "image", "source": {"type": "base64", "data": "AAAA"}}]}})
        .to_string(),
    ]
}

#[cfg(unix)]
#[test]
fn a_sending_row_with_zero_width_spaces_is_found_at_daemon_start_and_not_left_unsure() {
    let r = rig();
    let dir = Folder::new();
    let transcript = dir.0.join("projects/demo/fee7356b.jsonl");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    append(&transcript, &[other(-60_000)]);
    let job = json!({"state": "working", "tempo": "idle", "detail": "x", "template": "bg",
        "name": "main", "sessionId": "fee7356b-b891-4f6f-b4e7-35f7093b251c",
        "linkScanPath": transcript.to_string_lossy(), "cwd": "/work/demo",
        "updatedAt": "2026-11-14T22:54:49.559Z"});
    let state = dir.0.join("claude/jobs/be50d36a/state.json");
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
    let sentence = WORDS.join(&ZWSP.to_string());
    let text = format!("why this lamp \n{sentence}\n\n'{}'", dir.shot().display());
    let stored = format!("[Image #1]why this lamp \n{}", WORDS.concat());
    let new = New {
        session: "be50d36a".into(),
        tool: Tool::Claude,
        text,
        options: None,
        via: Via::Chat,
        clear_box: false,
    };
    let row = ops.store().insert(&new, T0 - 1_000).unwrap();
    ops.store().take(&row.id, T0).unwrap().unwrap();
    ops.store().set_seen(&row.id, Seen::default()).unwrap();
    append(&transcript, &absorbed(3_000, &stored));
    ops.load(T0 + 60_000);
    assert_eq!(
        ops.store()
            .get(&row.id)
            .unwrap()
            .map(|row| (row.state, row.why)),
        None,
        "Claude Code took the message before the daemon stopped: Sent, not May already be in"
    );
}

#[cfg(unix)]
#[test]
fn the_same_picture_message_without_invisible_characters_is_sent_the_same_way() {
    let e = env();
    let text = e.picture_message().replace(ZWSP, " ");
    let row = e.r.send("job-idle", &text);
    let stored = e.stored().replace(&WORDS.concat(), &WORDS.join(" "));
    e.turn_at(3_000, &stored);
    e.postmen.postman(IDLE).pass(e.r.at(T0));
    assert_eq!(e.state(&row.id), None);
    let gone = e.r.room.named("outbox.removed");
    assert_eq!(gone.last().unwrap()["why"], "sent");
}
