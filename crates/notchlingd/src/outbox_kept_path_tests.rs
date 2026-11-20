use super::*;
use crate::outbox_courier::Courier;
use crate::outbox_postman::{Couriers, Postmen};
use crate::outbox_testkit::{rig, Rig, IDLE, T0};
use notchling_engine::bridge::net::files::iso8601;
use notchling_engine::outbox::{State, Why};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

static FOLDERS: AtomicUsize = AtomicUsize::new(0);

struct Folder(PathBuf);

impl Folder {
    fn new() -> Folder {
        let n = FOLDERS.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("nl-outbox-kept-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Application Support").join("drops").join("k3x9"))
            .unwrap();
        Folder(dir)
    }

    fn shot(&self) -> PathBuf {
        let path = self
            .0
            .join("Application Support")
            .join("drops")
            .join("k3x9")
            .join("Screenshot 2026-11-20 at 10.14.52.png");
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

fn user(ms: i64, text: &str, images: usize) -> String {
    let mut blocks = vec![json!({"type": "text", "text": text})];
    for _ in 0..images {
        blocks.push(json!({"type": "image", "source": {"type": "base64", "data": "AAAA"}}));
    }
    json!({"type": "user", "uuid": "u", "timestamp": stamp(ms), "sessionId": "s-1",
        "cwd": "/work/demo", "origin": {"kind": "human"},
        "message": {"role": "user", "content": blocks}})
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
    append(&path, &[user(-60_000, "something else", 0)]);
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
    fn row_text(&self) -> String {
        format!("is it right\n\n'{}'", self.dir.shot().display())
    }

    fn line_at(&self, ms: i64, stored: &str, images: usize) {
        let lines = vec![user(ms, stored, images)];
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

    fn sent(&self) -> bool {
        let gone = self.r.room.named("outbox.removed");
        gone.last().is_some_and(|g| g["why"] == "sent")
    }
}

#[test]
fn a_picture_row_whose_path_stays_in_the_line_is_sent_when_the_line_shows_up() {
    let e = env();
    let text = e.row_text();
    let row = e.r.send("job-idle", &text);
    e.line_at(11_000, &text, 0);
    e.postmen.postman(IDLE).pass(e.r.at(T0));
    assert_eq!(e.state(&row.id), None, "Claude Code took it at 11 s");
    assert!(e.sent());
    assert_eq!(e.r.returns(), 1);
}

#[test]
fn the_same_line_with_the_newlines_folded_and_a_trailing_space_is_sent() {
    let e = env();
    let text = e.row_text();
    let row = e.r.send("job-idle", &text);
    let folded = format!("is it right  '{}' ", e.dir.shot().display());
    e.line_at(11_000, &folded, 0);
    e.postmen.postman(IDLE).pass(e.r.at(T0));
    assert_eq!(e.state(&row.id), None);
    assert!(e.sent());
}

#[test]
fn a_late_look_settles_the_picture_row_that_went_unsure() {
    let e = env();
    let text = e.row_text();
    let row = e.r.send("job-idle", &text);
    let mut postman = e.postmen.postman(IDLE);
    postman.pass(e.r.at(T0));
    assert_eq!(e.state(&row.id), Some((State::Unsure, Some(Why::NotFound))));
    append(&e.path, &[user(40_000, &text, 0)]);
    postman.pass(e.r.at(T0 + 45_000));
    assert_eq!(e.state(&row.id), None);
    assert!(e.sent());
    assert_eq!(e.r.returns(), 1, "never typed again");
}

#[test]
fn an_unsure_row_that_already_landed_is_settled_as_sent_and_not_stepped_aside() {
    let e = env();
    let text = e.row_text();
    let row = e.r.send("job-idle", &text);
    let mut postman = e.postmen.postman(IDLE);
    postman.pass(e.r.at(T0));
    assert_eq!(e.state(&row.id), Some((State::Unsure, Some(Why::NotFound))));
    append(&e.path, &[user(40_000, &text, 0)]);
    e.r.send("job-idle", "next message");
    postman.pass(e.r.at(T0 + 45_000));
    assert_eq!(e.state(&row.id), None);
    let gone = e.r.room.named("outbox.removed");
    assert!(
        gone.iter()
            .any(|g| g["id"] == json!(row.id) && g["why"] == "sent"),
        "{gone:?}"
    );
}
