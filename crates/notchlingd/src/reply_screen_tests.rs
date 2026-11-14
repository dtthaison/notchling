use crate::acting_ops_tests::{app, code, sessions, setup, PROMPT};
use crate::reply_screen::head;
use notchling_engine::reply::exec::Failure;
use notchling_engine::reply::fake::STARTING;
use notchling_engine::reply::keys;
use notchling_local_api::{ops, ErrorCode};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-reply-screen-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn reply(text: &str) -> serde_json::Value {
    json!({"id": "job-idle", "text": text})
}

#[test]
fn a_reply_to_a_session_that_is_starting_is_sent_once_its_prompt_is_up() {
    let s = setup(true, PROMPT, sessions());
    s.starting.store(2_000, Ordering::SeqCst);
    let out = s.ops.call(&app(), ops::SESSION_REPLY, &reply("hi"));
    assert_eq!(out.unwrap(), json!({"sent": true}));
    assert_eq!(
        *s.writes.lock().unwrap(),
        vec![
            keys::clear_input(),
            keys::bracketed("hi"),
            keys::ENTER.to_string()
        ],
        "each key once, none into the wait"
    );
}

#[test]
fn a_session_that_never_starts_gets_nothing_typed_and_says_it_did_not_open() {
    let dir = scratch("never");
    let file = dir.join("reply-screen.txt");
    let s = setup(true, PROMPT, sessions());
    let ops = s.ops.with_reply_screen_file(file.clone());
    s.starting.store(600_000, Ordering::SeqCst);
    let (c, m) = code(ops.call(&app(), ops::SESSION_REPLY, &reply("hi")));
    assert_eq!(c, ErrorCode::Failed);
    assert_eq!(m, "The session didn't open in time.");
    assert!(s.writes.lock().unwrap().is_empty());
    let kept = std::fs::read_to_string(&file).unwrap();
    assert!(
        kept.contains(&format!("await input: starting: {}", STARTING.trim())),
        "{kept}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_replys_screen_is_kept_private_without_the_message() {
    let dir = scratch("kept");
    let file = dir.join("reply-screen.txt");
    let s = setup(true, PROMPT, sessions());
    let ops = s.ops.with_reply_screen_file(file.clone());
    s.deaf.store(true, Ordering::SeqCst);
    let refused = ops.call(&app(), ops::SESSION_REPLY, &reply("tell nobody"));
    assert!(refused.is_err());
    let kept = std::fs::read_to_string(&file).unwrap();
    let lines: Vec<&str> = kept.lines().collect();
    assert_eq!(
        lines[..4],
        [
            "job: idle0001",
            "refused: The message is typed in the session's terminal but wasn't sent. Press Return there, or send it again.",
            "message: 11 characters",
            "cursor: row 1, column 13",
        ],
        "{kept}"
    );
    assert!(lines[4].starts_with("----- what each step saw ("), "{kept}");
    assert!(
        kept.contains("submit: input box, 1 row, holding text, cursor at 1,13 in the box"),
        "{kept}"
    );
    assert!(kept.contains("----- screen (4 lines) -----"), "{kept}");
    assert!(
        kept.contains("❯ ···· ······"),
        "the box keeps its shape: {kept}"
    );
    assert!(kept.contains("? for shortcuts"), "{kept}");
    assert!(!kept.contains("tell"), "never the message: {kept}");
    assert!(!kept.contains("nobody"), "{kept}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    std::fs::remove_file(&file).unwrap();
    s.deaf.store(false, Ordering::SeqCst);
    ops.call(&app(), ops::SESSION_REPLY, &reply("tell nobody"))
        .unwrap();
    assert!(!file.exists(), "nothing is kept when the reply went in");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_screen_with_no_prompt_is_kept_as_it_is() {
    let dir = scratch("other");
    let file = dir.join("reply-screen.txt");
    let s = setup(true, &["Resume a session", "  1. fix-hook"], sessions());
    let ops = s.ops.with_reply_screen_file(file.clone());
    assert!(ops.call(&app(), ops::SESSION_REPLY, &reply("hi")).is_err());
    let kept = std::fs::read_to_string(&file).unwrap();
    assert!(
        kept.contains("await input: no input box, cursor hidden, last row: 1. fix-hook"),
        "{kept}"
    );
    assert!(
        kept.ends_with("Resume a session\n  1. fix-hook\n"),
        "{kept}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_head_says_why_how_long_the_message_was_and_where_the_cursor_is() {
    let trail = vec!["+500ms await input: a dialog".to_string()];
    assert_eq!(
        head("ab12cd34", &Failure::Dialog, 33, None, &trail),
        [
            "job: ab12cd34",
            "refused: The session is asking you something. Answer it in the terminal, then send again.",
            "message: 33 characters",
            "cursor: hidden",
            "----- what each step saw (1 notes) -----",
            "+500ms await input: a dialog",
        ]
    );
}
