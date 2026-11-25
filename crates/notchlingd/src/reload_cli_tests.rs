use crate::cli::{parse, Command};
use crate::reload_cli::{parse as parse_reload, sentence, USAGE};
use serde_json::json;
use std::path::PathBuf;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| w.to_string()).collect()
}

#[test]
fn session_goes_to_its_own_command() {
    assert_eq!(
        parse(words(&["session", "reload", "abc", "--after-turn"])),
        Ok(Command::Session {
            args: words(&["reload", "abc", "--after-turn"])
        })
    );
}

#[test]
fn reload_takes_an_id_and_the_flags() {
    let args = parse_reload(&words(&[
        "reload",
        "abc",
        "--after-turn",
        "--json",
        "--socket",
        "/tmp/x.sock",
    ]))
    .unwrap();
    assert_eq!(args.id, "abc");
    assert!(args.after_turn && args.json);
    assert_eq!(args.socket, Some(PathBuf::from("/tmp/x.sock")));
    let plain = parse_reload(&words(&["reload", "self"])).unwrap();
    assert!(!plain.after_turn && !plain.json);
}

#[test]
fn reload_refuses_what_it_does_not_know() {
    assert_eq!(parse_reload(&words(&["reload"])), Err(USAGE.to_string()));
    assert_eq!(parse_reload(&words(&["stop", "a"])), Err(USAGE.to_string()));
    assert!(parse_reload(&words(&["reload", "a", "--now"]))
        .unwrap_err()
        .contains("unknown option"));
    assert!(parse_reload(&words(&["reload", "a", "b"])).is_err());
    assert!(parse_reload(&words(&["reload", "a", "--socket"])).is_err());
}

#[test]
fn the_reply_is_told_in_a_sentence() {
    let waiting = json!({"task": {"phase": "waiting", "by": {"why": "turn"}}});
    assert!(sentence(&waiting).contains("current turn ends"));
    let tasks = json!({"task": {"phase": "waiting", "by": {"why": "tasks", "count": 3}}});
    assert!(sentence(&tasks).contains("3 background tasks"));
    assert!(sentence(&json!({"task": {"phase": "done"}})).contains("reloaded"));
    assert!(sentence(&json!({"task": null})).contains("not started"));
}
