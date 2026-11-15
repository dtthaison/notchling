use crate::acting_ops_tests::{app, code, row, sessions, setup, Setup, PROMPT};
use notchling_engine::reply::fake::RULE;
use notchling_engine::reply::keys;
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

const FIRST: &str = "Please run the whole suite again and tell me what failed";

fn now() -> i64 {
    crate::acting_ops::now_ms()
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

fn leave(s: &Setup, text: &str) {
    *s.screen.lock().unwrap() = boxed(text);
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
    let s = setup(true, PROMPT, sessions());
    s.deaf.store(true, Ordering::SeqCst);
    assert!(s
        .ops
        .call(&app(), ops::SESSION_REPLY, &reply(FIRST))
        .is_err());
    s.writes.lock().unwrap().clear();
    s.deaf.store(false, Ordering::SeqCst);
    leave(&s, FIRST);
    let out = s.ops.call(&app(), ops::SESSION_REPLY, &reply("now fix it"));
    assert_eq!(out.unwrap(), json!({"sent": true}));
    assert_eq!(*s.writes.lock().unwrap(), typed("now fix it"));
}

#[test]
fn a_start_of_a_message_of_ours_is_ours_too() {
    let s = setup(true, PROMPT, sessions());
    s.deaf.store(true, Ordering::SeqCst);
    assert!(s
        .ops
        .call(&app(), ops::SESSION_REPLY, &reply(FIRST))
        .is_err());
    s.writes.lock().unwrap().clear();
    s.deaf.store(false, Ordering::SeqCst);
    leave(&s, "Please run the whole suite again");
    s.ops
        .call(&app(), ops::SESSION_REPLY, &reply("now fix it"))
        .unwrap();
    assert_eq!(*s.writes.lock().unwrap(), typed("now fix it"));
}

#[test]
fn a_message_of_ours_left_in_the_box_with_its_picture_as_a_chip_is_ours_too() {
    let with_picture =
        "ok\n\n'/Users/me/Library/Application Support/Notchling/drops/17/Screenshot 1.png'";
    let s = setup(true, PROMPT, sessions());
    s.deaf.store(true, Ordering::SeqCst);
    assert!(s
        .ops
        .call(&app(), ops::SESSION_REPLY, &reply(with_picture))
        .is_err());
    assert_eq!(s.ops.attempts.recent("job-idle", now()), [with_picture]);
    s.writes.lock().unwrap().clear();
    s.deaf.store(false, Ordering::SeqCst);
    leave(&s, "[Image #47]ok");
    let out = s.ops.call(&app(), ops::SESSION_REPLY, &reply("now fix it"));
    assert_eq!(out.unwrap(), json!({"sent": true}));
    assert_eq!(*s.writes.lock().unwrap(), typed("now fix it"));
}

#[test]
fn once_a_message_went_through_nothing_in_the_box_is_taken_for_ours_any_more() {
    let s = setup(true, PROMPT, sessions());
    s.deaf.store(true, Ordering::SeqCst);
    assert!(s
        .ops
        .call(&app(), ops::SESSION_REPLY, &reply(FIRST))
        .is_err());
    assert_eq!(s.ops.attempts.recent("job-idle", now()), [FIRST]);
    s.deaf.store(false, Ordering::SeqCst);
    leave(&s, FIRST);
    s.ops
        .call(&app(), ops::SESSION_REPLY, &reply("now fix it"))
        .unwrap();
    assert!(
        s.ops.attempts.recent("job-idle", now()).is_empty(),
        "the earlier try was cleared with the box and is gone"
    );
}

#[test]
fn another_sessions_leftover_is_not_ours_in_this_one() {
    let mut list = sessions();
    if let Some(rows) = list.as_array_mut() {
        rows.push(row(
            "job-two",
            json!({"job": "idle0002"}),
            "completed",
            None,
        ));
    }
    let s = setup(true, PROMPT, list);
    s.deaf.store(true, Ordering::SeqCst);
    let other = json!({"id": "job-two", "text": FIRST});
    assert!(s.ops.call(&app(), ops::SESSION_REPLY, &other).is_err());
    s.writes.lock().unwrap().clear();
    s.deaf.store(false, Ordering::SeqCst);
    leave(&s, FIRST);
    assert!(s
        .ops
        .call(&app(), ops::SESSION_REPLY, &reply("now fix it"))
        .is_err());
    assert!(s.writes.lock().unwrap().is_empty());
}

#[test]
fn a_draft_nobody_sent_is_refused_with_its_text_and_nothing_typed() {
    let dir = scratch("refused");
    let file = dir.join("reply-screen.txt");
    let s = setup(true, PROMPT, sessions());
    leave(&s, "my own draft, half written");
    let ops = s.ops.with_reply_screen_file(file.clone());
    let err = ops
        .call(&app(), ops::SESSION_REPLY, &reply("now fix it"))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert_eq!(
        err.message,
        "The session's input box holds other text, so the message wasn't sent."
    );
    assert_eq!(
        err.data,
        Some(json!({"why": "not_cleared", "box": "my own draft, half written"}))
    );
    assert!(s.writes.lock().unwrap().is_empty(), "not a key");
    let kept = std::fs::read_to_string(&file).unwrap();
    assert!(
        !kept.contains("draft") && !kept.contains("written"),
        "the file never holds the draft: {kept}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_long_draft_is_cut_before_it_is_sent_back() {
    let s = setup(true, PROMPT, sessions());
    leave(&s, &"word ".repeat(600));
    let err = s
        .ops
        .call(&app(), ops::SESSION_REPLY, &reply("now fix it"))
        .unwrap_err();
    let shown = err.data.unwrap()["box"].as_str().unwrap().to_owned();
    assert!(shown.ends_with('…'));
    assert_eq!(shown.chars().count(), crate::acting_ops::BOX_SHOWN_MAX + 1);
}

#[test]
fn asking_to_clear_the_box_clears_it_with_the_safe_keys_and_sends_once() {
    let s = setup(true, PROMPT, sessions());
    leave(&s, "my own draft, half written");
    let out = s
        .ops
        .call(&app(), ops::SESSION_REPLY, &clearing("now fix it"));
    assert_eq!(out.unwrap(), json!({"sent": true}));
    let writes = s.writes.lock().unwrap().clone();
    assert_eq!(writes, typed("now fix it"));
    for never in ['\x0b', '\x03', '\x1b'] {
        assert!(!writes[0].contains(never), "the clear keys: {never:?}");
    }
}

#[test]
fn asking_to_clear_with_nothing_in_the_box_just_sends() {
    let s = setup(true, PROMPT, sessions());
    let out = s
        .ops
        .call(&app(), ops::SESSION_REPLY, &clearing("now fix it"));
    assert_eq!(out.unwrap(), json!({"sent": true}));
    assert_eq!(*s.writes.lock().unwrap(), typed("now fix it"));
}

#[test]
fn a_clear_that_does_not_take_sends_nothing_and_shows_what_is_left() {
    let s = setup(true, PROMPT, sessions());
    leave(&s, "my own draft");
    s.human.lock().unwrap().push((550, "x".into()));
    let err = s
        .ops
        .call(&app(), ops::SESSION_REPLY, &clearing("now fix it"))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert_eq!(
        err.data,
        Some(json!({"why": "not_cleared", "box": "x"})),
        "what is in the box now"
    );
    assert_eq!(
        *s.writes.lock().unwrap(),
        [keys::clear_input()],
        "no paste and no Enter"
    );
}

#[test]
fn clear_box_is_true_or_false_and_nothing_else() {
    let s = setup(true, PROMPT, sessions());
    for bad in [json!("yes"), json!(1), json!([true])] {
        let args = json!({"id": "job-idle", "text": "hi", "clear_box": bad});
        let (code, _) = code(s.ops.call(&app(), ops::SESSION_REPLY, &args));
        assert_eq!(code, ErrorCode::BadRequest);
    }
    let args = json!({"id": "job-idle", "text": "hi", "clear_box": null});
    assert!(s.ops.call(&app(), ops::SESSION_REPLY, &args).is_ok());
    assert!(s.attached.lock().unwrap().len() == 1, "only the good call");
}
