use crate::acting_ops_tests::{app, setup, Setup, PROMPT};
use crate::held_race_tests::{
    assistant, job_file, lines, live_file, prompt_line, say, session, set_row, turn_end, typed, Dir,
};
use notchling_engine::reply::held::{BLOCKED_RETRY_MS, SETTLE_MS, START_GRACE_MS};
use notchling_engine::reply::keys::timing::{ATTACH_QUIET_MS, KEY_PAUSE_MS};
use notchling_engine::reply::keys::ENTER;
use notchling_local_api::ops;
use serde_json::{json, Value};

const T0: i64 = 1_794_697_060_000;
const ID: &str = "fee7356b-b891-4f6f-b4e7-35f7093b251c";
const BOX: &str = "Waiting — the terminal box has other text";

const DRAFT: &[&str] = &[
    "────────────────────────────────────────",
    "❯ my own draft",
    "────────────────────────────────────────",
    "  ? for shortcuts",
];

fn ended() -> Vec<String> {
    vec![prompt_line("go"), assistant("done"), turn_end()]
}

fn held_after_a_turn(tag: &str) -> (Dir, Setup) {
    let dir = Dir::new(tag);
    lines(&dir, &ended());
    job_file(&dir, "done", 3, "done");
    live_file(&dir, "busy");
    let s = session(&dir, "working", false);
    assert_eq!(say(&s, "two")["state"], json!("queued"));
    set_row(&s, &dir, "completed", false);
    (dir, s)
}

fn flush(s: &Setup, at: i64) {
    s.ops.flush_held(at);
    s.ops.flush_held(at + SETTLE_MS);
}

fn returns(s: &Setup) -> usize {
    s.writes
        .lock()
        .unwrap()
        .iter()
        .filter(|w| w.as_str() == ENTER)
        .count()
}

fn last_held(s: &Setup) -> Value {
    s.room
        .named("session.held")
        .last()
        .cloned()
        .unwrap_or(Value::Null)
}

fn a_person_types_beside_the_flush(s: &Setup) {
    s.human
        .lock()
        .unwrap()
        .push((ATTACH_QUIET_MS + KEY_PAUSE_MS + 10, "x".to_string()));
}

fn after(at: i64) -> i64 {
    at + SETTLE_MS + BLOCKED_RETRY_MS
}

#[test]
fn a_flush_that_meets_a_persons_typing_keeps_the_text_shows_it_and_presses_no_return() {
    let (_dir, s) = held_after_a_turn("kept-mixed");
    a_person_types_beside_the_flush(&s);
    flush(&s, T0 + 1_000);
    assert_eq!(returns(&s), 0, "no Return on a mix");
    assert_eq!(s.ops.held_text(ID).as_deref(), Some("two"), "not dropped");
    let shown = last_held(&s);
    assert_eq!(shown["id"], json!(ID));
    assert_eq!(shown["text"], json!("two"));
    let note = shown["note"].as_str().expect("a state is shown");
    assert!(note.contains("typed"), "{note}");
}

#[test]
fn it_is_typed_once_the_box_is_empty_again() {
    let (_dir, s) = held_after_a_turn("kept-clears");
    a_person_types_beside_the_flush(&s);
    flush(&s, T0 + 1_000);
    s.human.lock().unwrap().clear();
    flush(&s, after(T0 + 1_000));
    assert_eq!(returns(&s), 1, "sent once");
    assert_eq!(s.ops.held_text(ID), None);
    let shown = last_held(&s);
    assert_eq!(shown["text"], Value::Null);
    assert_eq!(shown.get("note"), None, "the state goes with the text");
    flush(&s, after(after(T0 + 1_000)));
    assert_eq!(returns(&s), 1, "never again");
}

#[test]
fn text_a_person_sent_along_with_ours_is_not_typed_again_when_the_box_empties() {
    let (dir, s) = held_after_a_turn("kept-sent");
    a_person_types_beside_the_flush(&s);
    flush(&s, T0 + 1_000);
    let mut all = ended();
    all.push(
        json!({"type": "user", "timestamp": "2026-11-14T22:57:50.000Z",
            "message": {"role": "user", "content": "my own words two"}})
        .to_string(),
    );
    all.push(assistant("on it"));
    all.push(turn_end());
    lines(&dir, &all);
    s.human.lock().unwrap().clear();
    let attaches = s.attached.lock().unwrap().len();
    let writes = s.writes.lock().unwrap().len();
    flush(&s, after(T0 + 1_000));
    assert_eq!(
        s.attached.lock().unwrap().len(),
        attaches,
        "no need to look"
    );
    assert_eq!(s.writes.lock().unwrap().len(), writes, "nothing typed");
    assert_eq!(returns(&s), 0);
    assert_eq!(s.ops.held_text(ID), None, "it went with theirs");
    assert_eq!(last_held(&s)["text"], Value::Null);
}

#[test]
fn when_the_transcript_cannot_say_it_stays_held_untyped_and_says_so() {
    let (dir, s) = held_after_a_turn("kept-unsure");
    a_person_types_beside_the_flush(&s);
    flush(&s, T0 + 1_000);
    std::fs::remove_file(dir.path("transcript.jsonl")).unwrap();
    s.human.lock().unwrap().clear();
    let writes = s.writes.lock().unwrap().len();
    let mut at = T0 + 1_000;
    for _ in 0..4 {
        at = after(at);
        flush(&s, at);
    }
    assert_eq!(s.writes.lock().unwrap().len(), writes, "never typed again");
    assert_eq!(returns(&s), 0);
    assert_eq!(s.ops.held_text(ID).as_deref(), Some("two"));
    let note = last_held(&s)["note"].as_str().unwrap().to_owned();
    assert!(note.contains("already"), "{note}");
}

#[test]
fn text_sent_meanwhile_joins_the_held_text_and_only_the_new_part_goes_when_ours_went_with_theirs() {
    let (dir, s) = held_after_a_turn("kept-joined");
    a_person_types_beside_the_flush(&s);
    flush(&s, T0 + 1_000);
    assert_eq!(say(&s, "three")["held"], json!("two\n\nthree"));
    let mut all = ended();
    all.push(
        json!({"type": "user", "timestamp": "2026-11-14T22:57:50.000Z",
            "message": {"role": "user", "content": "x two"}})
        .to_string(),
    );
    all.push(turn_end());
    lines(&dir, &all);
    s.human.lock().unwrap().clear();
    let before = s.writes.lock().unwrap().len();
    flush(&s, after(T0 + 1_000));
    let fresh: Vec<String> = s.writes.lock().unwrap()[before..].to_vec();
    let paste = fresh.iter().find(|w| w.contains("three")).expect("a paste");
    assert!(!paste.contains("two"), "only the new part: {paste:?}");
    assert_eq!(returns(&s), 1);
    assert_eq!(s.ops.held_text(ID), None);
}

#[test]
fn a_draft_in_the_box_keeps_the_text_shown_with_the_state_and_it_goes_when_the_box_is_empty() {
    let s = setup(true, DRAFT, crate::acting_ops_tests::sessions());
    say_job_a(&s, "two");
    at_rest(&s);
    flush(&s, T0 + 1_000);
    assert!(typed(&s).is_empty(), "not even the clear keys");
    assert_eq!(s.ops.held_text("job-a").as_deref(), Some("two"));
    let shown = last_held(&s);
    assert_eq!(shown["text"], json!("two"));
    assert_eq!(shown["note"], json!(BOX));
    flush(&s, T0 + 1_000 + START_GRACE_MS);
    assert_eq!(
        s.attached.lock().unwrap().len(),
        1,
        "not looked at again at once"
    );
    *s.screen.lock().unwrap() = PROMPT.iter().map(|l| l.to_string()).collect();
    flush(&s, after(T0 + 1_000));
    assert_eq!(s.attached.lock().unwrap().len(), 2);
    assert_eq!(returns(&s), 1);
    assert!(typed(&s).contains("two"));
    assert_eq!(s.ops.held_text("job-a"), None);
    assert_eq!(last_held(&s).get("note"), None);
}

#[test]
fn a_user_can_discard_a_text_that_is_waiting_and_it_is_never_typed() {
    let s = setup(true, DRAFT, crate::acting_ops_tests::sessions());
    say_job_a(&s, "two");
    at_rest(&s);
    flush(&s, T0 + 1_000);
    let out = s
        .ops
        .call(&app(), ops::SESSION_HELD_DISCARD, &json!({"id": "job-a"}))
        .unwrap();
    assert_eq!(out, json!({"discarded": true}));
    assert_eq!(s.ops.held_text("job-a"), None);
    let shown = last_held(&s);
    assert_eq!(shown["text"], Value::Null);
    *s.screen.lock().unwrap() = PROMPT.iter().map(|l| l.to_string()).collect();
    flush(&s, after(T0 + 1_000));
    assert!(typed(&s).is_empty());
    let again = s
        .ops
        .call(&app(), ops::SESSION_HELD_DISCARD, &json!({"id": "job-a"}))
        .unwrap();
    assert_eq!(again, json!({"discarded": false}));
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

fn at_rest(s: &Setup) {
    let mut list = s.ops.sessions.lock().unwrap();
    for r in list.as_array_mut().unwrap() {
        if r["id"] == json!("job-a") {
            r["group"] = json!("completed");
        }
    }
}
