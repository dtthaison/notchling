use crate::acting_ops_tests::{app, code};
use crate::outbox_testkit::{rig, rig_with, Options, Rig, T0};
use notchling_engine::outbox::Verdict;
use notchling_engine::reply::fake::RULE;
use notchling_engine::reply::keys;
use notchling_local_api::{ops, ApiError, ErrorCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

const FIRST: &str = "Please run the whole suite again and tell me what failed";

fn setup() -> Rig {
    let r = rig();
    r.threaded();
    r
}

fn call(r: &Rig, args: &Value) -> Result<Value, ApiError> {
    r.acting.call(&app(), ops::SESSION_REPLY, args)
}

fn unheard(r: &Rig, args: &Value) {
    r.deaf.store(true, Ordering::SeqCst);
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    assert!(call(r, args).is_err());
    r.deaf.store(false, Ordering::SeqCst);
}

fn reply(text: &str) -> Value {
    json!({"id": "job-idle", "text": text})
}

fn clearing(text: &str) -> Value {
    json!({"id": "job-idle", "text": text, "clear_box": true})
}

fn boxed(text: &str) -> Vec<String> {
    let row = format!("❯ {text}");
    [RULE, "⏺ ok", RULE, row.as_str(), RULE, "  ? for shortcuts"]
        .iter()
        .map(|l| l.to_string())
        .collect()
}

fn leave(r: &Rig, text: &str) {
    *r.screen.lock().unwrap() = boxed(text);
}

fn typed(text: &str) -> Vec<String> {
    vec![
        keys::clear_input(),
        keys::bracketed(text),
        keys::ENTER.to_string(),
    ]
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-reply-clear-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_message_of_ours_left_in_the_box_is_cleared_and_the_next_one_goes_without_asking() {
    let r = setup();
    unheard(&r, &reply(FIRST));
    r.writes.lock().unwrap().clear();
    leave(&r, FIRST);
    let out = call(&r, &reply("now fix it"));
    assert_eq!(out.unwrap(), json!({"sent": true}));
    assert_eq!(*r.writes.lock().unwrap(), typed("now fix it"));
}

#[test]
fn a_start_of_a_message_of_ours_is_ours_too() {
    let r = setup();
    unheard(&r, &reply(FIRST));
    r.writes.lock().unwrap().clear();
    leave(&r, "Please run the whole suite again");
    call(&r, &reply("now fix it")).unwrap();
    assert_eq!(*r.writes.lock().unwrap(), typed("now fix it"));
}

#[test]
fn a_picture_the_person_pasted_beside_words_like_ours_is_not_cleared_as_ours() {
    let r = setup();
    unheard(&r, &reply(FIRST));
    r.writes.lock().unwrap().clear();
    leave(&r, &format!("[Image #5]{FIRST}"));
    let err = call(&r, &reply("now fix it")).unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert!(r.writes.lock().unwrap().is_empty(), "not a key");
}

#[test]
fn a_picture_the_person_pasted_is_not_cleared_as_a_chip_our_words_name() {
    let named = "why does [Image #1] fail?";
    let r = setup();
    unheard(&r, &reply(named));
    r.writes.lock().unwrap().clear();
    leave(&r, "[Image #5]why does fail?");
    let err = call(&r, &reply("now fix it")).unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert!(r.writes.lock().unwrap().is_empty(), "not a key");
}

#[test]
fn a_message_of_ours_left_in_the_box_with_its_picture_as_a_chip_is_ours_too() {
    let with_picture =
        "ok\n\n'/Users/me/Library/Application Support/Notchling/drops/17/Screenshot 1.png'";
    let r = setup();
    unheard(&r, &reply(with_picture));
    assert_eq!(r.acting.attempts.recent("job-idle", T0), [with_picture]);
    r.writes.lock().unwrap().clear();
    leave(&r, "[Image #47]ok");
    let out = call(&r, &reply("now fix it"));
    assert_eq!(out.unwrap(), json!({"sent": true}));
    assert_eq!(*r.writes.lock().unwrap(), typed("now fix it"));
}

#[test]
fn once_a_message_went_through_nothing_in_the_box_is_taken_for_ours_any_more() {
    let r = setup();
    unheard(&r, &reply(FIRST));
    assert_eq!(r.acting.attempts.recent("job-idle", T0), [FIRST]);
    leave(&r, FIRST);
    call(&r, &reply("now fix it")).unwrap();
    assert!(
        r.acting.attempts.recent("job-idle", T0).is_empty(),
        "the earlier try was cleared with the box and is gone"
    );
}

#[test]
fn another_sessions_leftover_is_not_ours_in_this_one() {
    let r = setup();
    unheard(&r, &json!({"id": "job-idle2", "text": FIRST}));
    r.writes.lock().unwrap().clear();
    leave(&r, FIRST);
    assert!(call(&r, &reply("now fix it")).is_err());
    assert!(r.writes.lock().unwrap().is_empty());
}

#[test]
fn a_draft_nobody_sent_is_refused_with_its_text_and_nothing_typed() {
    let dir = scratch("refused");
    let file = dir.join("reply-screen.txt");
    let r = rig_with(Options {
        reply_screen: Some(file.clone()),
        ..Options::default()
    });
    r.threaded();
    leave(&r, "my own draft, half written");
    let err = call(&r, &reply("now fix it")).unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert_eq!(
        err.message,
        "The session's input box holds other text, so the message wasn't sent."
    );
    assert_eq!(
        err.data,
        Some(json!({"why": "not_cleared", "box": "my own draft, half written"}))
    );
    assert!(r.writes.lock().unwrap().is_empty(), "not a key");
    let kept = std::fs::read_to_string(&file).unwrap();
    assert!(
        !kept.contains("draft") && !kept.contains("written"),
        "the file never holds the draft: {kept}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_long_draft_is_cut_before_it_is_sent_back() {
    let r = setup();
    leave(&r, &"word ".repeat(600));
    let err = call(&r, &reply("now fix it")).unwrap_err();
    let shown = err.data.unwrap()["box"].as_str().unwrap().to_owned();
    assert!(shown.ends_with('…'));
    assert_eq!(shown.chars().count(), crate::acting_ops::BOX_SHOWN_MAX + 1);
}

#[test]
fn asking_to_clear_the_box_clears_it_with_the_safe_keys_and_sends_once() {
    let r = setup();
    leave(&r, "my own draft, half written");
    let out = call(&r, &clearing("now fix it"));
    assert_eq!(out.unwrap(), json!({"sent": true}));
    let writes = r.writes.lock().unwrap().clone();
    assert_eq!(writes, typed("now fix it"));
    for never in ['\x0b', '\x03', '\x1b'] {
        assert!(!writes[0].contains(never), "the clear keys: {never:?}");
    }
}

#[test]
fn asking_to_clear_with_nothing_in_the_box_just_sends() {
    let r = setup();
    let out = call(&r, &clearing("now fix it"));
    assert_eq!(out.unwrap(), json!({"sent": true}));
    assert_eq!(*r.writes.lock().unwrap(), typed("now fix it"));
}

#[test]
fn a_clear_that_does_not_take_sends_nothing_and_shows_what_is_left() {
    let r = setup();
    leave(&r, "my own draft");
    r.human.lock().unwrap().push((550, "x".into()));
    let err = call(&r, &clearing("now fix it")).unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert_eq!(
        err.data,
        Some(json!({"why": "not_cleared", "box": "x"})),
        "what is in the box now"
    );
    assert_eq!(
        *r.writes.lock().unwrap(),
        [keys::clear_input()],
        "no paste and no Enter"
    );
}

#[test]
fn clear_box_is_true_or_false_and_nothing_else() {
    let r = setup();
    for bad in [json!("yes"), json!(1), json!([true])] {
        let args = json!({"id": "job-idle", "text": "hi", "clear_box": bad});
        let (code, _) = code(call(&r, &args));
        assert_eq!(code, ErrorCode::BadRequest);
    }
    let args = json!({"id": "job-idle", "text": "hi", "clear_box": null});
    assert!(call(&r, &args).is_ok());
    assert!(r.attaches() == 1, "only the good call");
}
