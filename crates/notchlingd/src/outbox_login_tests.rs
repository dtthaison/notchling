use crate::outbox_courier::{Courier, NOTHING};
use crate::outbox_login::hold;
use crate::outbox_postman::Next;
use crate::outbox_rows::wire;
use crate::outbox_testkit::{rig, rig_with, Options, Rig, IDLE, SWITCH_MODEL, T0};
use notchling_engine::outbox::{
    Facts, Gate, Job, Kind, Row, State, Tried, TryEnd, Verdict, Why, LOGIN_RECHECK_MS,
    TURN_SETTLE_MS,
};
use notchling_engine::reply::exec::Failure;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const EXPIRED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../engine/tests/fixtures/notchling/login-expired.jsonl"
);
const SIGNED_IN_LINE: &str = r#"{"type":"user","uuid":"u-9","isSidechain":false,"timestamp":"2026-11-21T10:31:20.000Z","message":{"role":"user","content":"<local-command-stdout>Login successful</local-command-stdout>"}}"#;

struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn signed_out_rig(tag: &str) -> (Rig, Folder, PathBuf) {
    let dir = std::env::temp_dir().join(format!("nl-outbox-login-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let folder = Folder(dir.clone());
    let transcript = dir.join("session.jsonl");
    std::fs::copy(EXPIRED, &transcript).unwrap();
    let r = rig();
    r.files
        .rows
        .lock()
        .unwrap()
        .get_mut(IDLE)
        .unwrap()
        .transcript_path = Some(transcript.to_string_lossy().into_owned());
    (r, folder, transcript)
}

fn append(path: &PathBuf, line: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{line}").unwrap();
}

fn parked(r: &Rig, id: &str) -> Option<(State, Option<Why>)> {
    r.row(id).map(|row| (row.state, row.why))
}

#[test]
fn a_message_to_a_signed_out_session_is_held_not_typed_and_goes_after_login() {
    let (r, _folder, transcript) = signed_out_rig("hold");
    let row = r.send("job-idle", "go on please");
    let mut p = r.postmen.postman(IDLE);

    assert_eq!(p.pass(r.at(T0)), Next::At(T0 + LOGIN_RECHECK_MS));
    assert_eq!(r.typed(), "", "nothing is typed while signed out");
    assert_eq!(r.attaches(), 0);
    assert_eq!(parked(&r, &row.id), Some((State::Waiting, None)));
    let shown = wire(&r.row(&row.id).unwrap(), 1, None, r.ops.shown(&row.id));
    assert_eq!(shown["state"], "waiting");
    assert_eq!(shown["why"], "login_needed");

    assert_eq!(
        p.pass(r.at(T0 + LOGIN_RECHECK_MS)),
        Next::At(T0 + 2 * LOGIN_RECHECK_MS)
    );
    assert_eq!(r.typed(), "", "still held on the next look");

    append(&transcript, SIGNED_IN_LINE);
    p.pass(r.at(T0 + 2 * LOGIN_RECHECK_MS));
    assert!(r.typed().contains("go on please"), "{:?}", r.typed());
    assert_eq!(r.row(&row.id), None, "sent and removed");
    assert_eq!(r.ops.shown(&row.id), None);
}

#[test]
fn a_slash_command_is_not_held_for_the_login() {
    let (r, _folder, _) = signed_out_rig("command");
    let row = r.send("job-idle", "/model claude-opus-5-5");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_ne!(
        parked(&r, &row.id),
        Some((State::Waiting, None)),
        "the command is judged by its own rules, not held as signed out"
    );
    assert_eq!(r.ops.shown(&row.id), None);
}

#[test]
fn a_session_that_is_signed_in_is_not_held() {
    let (r, _folder, transcript) = signed_out_rig("signedin");
    append(&transcript, SIGNED_IN_LINE);
    let row = r.send("job-idle", "carry on");
    r.postmen.postman(IDLE).pass(r.at(T0));
    assert!(r.typed().contains("carry on"));
    assert_eq!(r.row(&row.id), None);
}

struct SignedOut {
    out: bool,
    asked: AtomicUsize,
}

impl Courier for SignedOut {
    fn facts(&self, _row: &Row, now_ms: i64) -> Facts {
        Facts {
            now_ms,
            job: Job::Live { status: None },
            dialog_on_screen: false,
            queue_waiting: 0,
            safe: false,
            turn_over_since_ms: None,
        }
    }
    fn deliver(&self, _row: &Row, _guarded: bool) -> (TryEnd, Tried, Option<String>) {
        (TryEnd::Ok, NOTHING, None)
    }
    fn close(&self, _session: &str) {}
    fn verifies(&self) -> bool {
        false
    }
    fn signed_out(&self, _row: &Row) -> bool {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.out
    }
}

fn row_of(kind: Kind, why: Option<Why>) -> Row {
    let r = rig();
    let mut row = r.send("job-idle", "hello");
    row.kind = kind;
    row.why = why;
    row
}

#[test]
fn only_a_go_for_a_plain_message_turns_into_the_login_wait() {
    let signed_out = SignedOut {
        out: true,
        asked: AtomicUsize::new(0),
    };
    let go = Gate::Go { guarded: false };
    let held = hold(go.clone(), &row_of(Kind::Prompt, None), &signed_out, T0);
    assert_eq!(
        held,
        Gate::Wait {
            why: Why::LoginNeeded,
            until_ms: T0 + LOGIN_RECHECK_MS
        }
    );
    let command = hold(go.clone(), &row_of(Kind::Command, None), &signed_out, T0);
    assert_eq!(command, go);
    let answer = hold(
        go.clone(),
        &row_of(Kind::Prompt, Some(Why::Answer)),
        &signed_out,
        T0,
    );
    assert_eq!(answer, go);
    let others = [
        Gate::Wait {
            why: Why::Dialog,
            until_ms: T0 + 1,
        },
        Gate::NeedsUser(State::Stopped),
        Gate::Delivered,
    ];
    for other in others {
        let kept = hold(other.clone(), &row_of(Kind::Prompt, None), &signed_out, T0);
        assert_eq!(kept, other);
    }
}

#[test]
fn a_session_that_is_not_signed_out_keeps_its_go() {
    let signed_in = SignedOut {
        out: false,
        asked: AtomicUsize::new(0),
    };
    let go = Gate::Go { guarded: true };
    let kept = hold(go.clone(), &row_of(Kind::Prompt, None), &signed_in, T0);
    assert_eq!(kept, go);
    assert_eq!(signed_in.asked.load(Ordering::SeqCst), 1);
}

#[test]
fn the_default_courier_never_says_signed_out() {
    let r = rig();
    let row = r.send("job-idle", "hello");
    let refuse = crate::outbox_courier::Refuse;
    assert!(!refuse.signed_out(&row));
}

struct Failing {
    out: bool,
    delivered: AtomicUsize,
}

impl Failing {
    fn new(out: bool) -> Arc<Failing> {
        Arc::new(Failing {
            out,
            delivered: AtomicUsize::new(0),
        })
    }

    fn delivered(&self) -> usize {
        self.delivered.load(Ordering::SeqCst)
    }
}

impl Courier for Failing {
    fn facts(&self, _row: &Row, now_ms: i64) -> Facts {
        Facts {
            now_ms,
            job: Job::Live { status: None },
            dialog_on_screen: false,
            queue_waiting: 0,
            safe: false,
            turn_over_since_ms: Some(0),
        }
    }
    fn deliver(&self, _row: &Row, _guarded: bool) -> (TryEnd, Tried, Option<String>) {
        self.delivered.fetch_add(1, Ordering::SeqCst);
        (TryEnd::Failed(Failure::LoginNeeded), NOTHING, None)
    }
    fn close(&self, _session: &str) {}
    fn verifies(&self) -> bool {
        false
    }
    fn signed_out(&self, _row: &Row) -> bool {
        self.out
    }
}

fn fails_signed_out_then_waits(out: bool, text: &str) {
    let failing = Failing::new(out);
    let r = rig_with(Options {
        courier: Some(failing.clone()),
        ..Options::default()
    });
    let row = r.send("job-idle", text);
    let mut p = r.postmen.postman(IDLE);

    p.pass(r.at(T0));
    assert_eq!(failing.delivered(), 1, "{text}");
    let waiting = r.row(&row.id).unwrap();
    assert_eq!(waiting.state, State::Waiting, "{text}");
    assert_eq!(waiting.why, None, "the database never holds login_needed");
    assert_eq!(waiting.next_ms, Some(T0 + LOGIN_RECHECK_MS), "{text}");
    assert_eq!(r.ops.shown(&row.id), Some(Why::LoginNeeded));
    let on_the_wire = wire(&waiting, 1, None, r.ops.shown(&row.id));
    assert_eq!(on_the_wire["why"], "login_needed");

    p.pass(r.at(T0 + 1_000));
    assert_eq!(failing.delivered(), 1, "not tried again at once: {text}");
    p.pass(r.at(T0 + LOGIN_RECHECK_MS - 1));
    assert_eq!(failing.delivered(), 1, "{text}");
    p.pass(r.at(T0 + LOGIN_RECHECK_MS));
    assert_eq!(failing.delivered(), 2, "tried again at the recheck: {text}");
}

#[test]
fn a_slash_command_that_failed_signed_out_waits_for_the_recheck() {
    fails_signed_out_then_waits(true, "/model claude-opus-5-5");
}

#[test]
fn a_message_that_failed_signed_out_waits_for_the_recheck() {
    fails_signed_out_then_waits(false, "carry on");
}

#[test]
fn an_answer_row_stays_an_answer_while_the_session_is_signed_out() {
    let (r, _folder, _) = signed_out_rig("answer");
    r.open_dialog_after_a_command(SWITCH_MODEL);
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let row = r.send("job-idle", "/model claude-opus-5-5");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    let typed_at = r.at(T0 + TURN_SETTLE_MS);
    p.pass(typed_at);
    assert_eq!(r.returns(), 1);
    assert_eq!(
        parked(&r, &row.id),
        Some((State::Waiting, Some(Why::Answer)))
    );

    r.files.set_status(IDLE, "waiting");
    for step in 1..=3 {
        p.pass(r.at(typed_at + step * 3_000));
        assert_eq!(
            parked(&r, &row.id),
            Some((State::Waiting, Some(Why::Answer))),
            "pass {step}"
        );
        assert_eq!(r.ops.shown(&row.id), None, "pass {step}");
        assert_eq!(r.returns(), 1, "never typed again, pass {step}");
    }

    r.files.set_status(IDLE, "idle");
    p.pass(r.at(typed_at + 12_000));
    assert_eq!(r.row(&row.id), None, "answered, so the row is done");
    assert_eq!(r.returns(), 1);
}

#[test]
fn the_login_word_adds_no_key_to_the_wire_row_and_none_to_the_database() {
    let r = rig();
    let row = r.send("job-idle", "hello");
    let waiting = Row {
        state: State::Waiting,
        why: Why::LoginNeeded.stored(),
        ..row
    };
    assert_eq!(waiting.why, None, "an older daemon reads the database row");
    let held = wire(&waiting, 1, None, Some(Why::LoginNeeded));
    let checking = wire(&waiting, 1, None, Some(Why::Checking));
    let keys = |value: &serde_json::Value| -> Vec<String> {
        value.as_object().unwrap().keys().cloned().collect()
    };
    assert_eq!(keys(&held), keys(&checking));
    assert_eq!(held["why"], "login_needed");
    assert_eq!(checking["why"], "checking");
    let bare = wire(&waiting, 1, None, None);
    assert!(bare.get("why").is_none(), "no why when nothing is shown");
}
