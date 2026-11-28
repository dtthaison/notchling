use super::*;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn new_takes_a_folder_a_model_and_a_first_message() {
    let a = parse(&words(&[
        "new",
        "/work/app",
        "--model",
        "sonnet",
        "--message",
        "run the tests",
        "--reason",
        "needs a clean run",
        "--no-wait",
        "--json",
    ]))
    .unwrap();
    assert_eq!(a.verb, Verb::New);
    assert!(a.json && !a.flags.wait);
    assert_eq!(
        request_body(&a),
        json!({"folder": "/work/app", "model": "sonnet", "message": "run the tests",
               "consent": {"reason": "needs a clean run"}})
    );
    assert_eq!(a.verb.op(), ops::SESSION_START);
}

#[test]
fn reply_stop_interrupt_and_remove_name_a_session() {
    let reply = parse(&words(&[
        "reply",
        "abc",
        "hello there",
        "--expires-in",
        "90",
    ]))
    .unwrap();
    assert_eq!(
        request_body(&reply),
        json!({"id": "abc", "text": "hello there", "consent": {"expiresInMs": 90_000}})
    );
    assert_eq!(reply.verb.op(), ops::SESSION_SAY);
    let stop = parse(&words(&["stop", "abc"])).unwrap();
    assert_eq!(request_body(&stop)["how"], json!("stop"));
    let interrupt = parse(&words(&["interrupt", "abc"])).unwrap();
    assert_eq!(request_body(&interrupt)["how"], json!("interrupt"));
    assert_eq!(interrupt.verb.op(), ops::SESSION_HALT);
    let remove = parse(&words(&["remove", "abc", "--dry-run"])).unwrap();
    assert_eq!(remove.verb.op(), ops::SESSION_DISCARD);
    assert_eq!(request_body(&remove)["dryRun"], json!(true));
}

#[test]
fn a_double_dash_makes_the_rest_the_text() {
    let reply = parse(&words(&[
        "reply",
        "abc",
        "--no-wait",
        "--",
        "--keep",
        "going",
    ]))
    .unwrap();
    assert_eq!(reply.text.as_deref(), Some("--keep going"));
    assert!(!reply.flags.wait);
    let new = parse(&words(&["new", "/work/app", "--", "start with the tests"])).unwrap();
    assert_eq!(new.text.as_deref(), Some("start with the tests"));
    assert!(parse(&words(&["stop", "abc", "--", "now"])).is_err());
    assert!(parse(&words(&["reply", "abc", "x", "--", "y"])).is_err());
}

#[test]
fn a_missing_word_or_a_stray_option_is_usage() {
    for bad in [
        vec![],
        vec!["new"],
        vec!["reply", "abc"],
        vec!["stop"],
        vec!["stop", "a", "b"],
        vec!["fly", "abc"],
        vec!["stop", "abc", "--odd"],
        vec!["stop", "abc", "--model", "x"],
        vec!["new", "/f", "--timeout", "soon"],
    ] {
        assert!(parse(&words(&bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn the_done_line_names_the_session() {
    assert_eq!(done_line(Verb::Stop, true, "s1"), "Stopped the session s1.");
    assert_eq!(
        done_line(Verb::New, false, ""),
        "Started the session (the user said yes)."
    );
}

#[test]
fn answer_command_dialog_chrome_and_options_name_a_session_and_their_text() {
    let answer = parse(&words(&[
        "answer",
        "abc",
        "--answers",
        "[[2],\"free text\"]",
        "--question",
        "[{\"question\": \"Which db?\", \"options\": [\"a\", \"b\"]}]",
    ]))
    .unwrap();
    assert_eq!(answer.verb.op(), ops::SESSION_CHOOSE);
    assert_eq!(
        request_body(&answer),
        json!({"id": "abc", "answers": [[2], "free text"],
               "question": {"items": [{"question": "Which db?", "options": ["a", "b"]}]},
               "consent": {}})
    );
    let command = parse(&words(&[
        "command", "abc", "--", "/compact", "keep", "tests",
    ]))
    .unwrap();
    assert_eq!(command.verb.op(), ops::SESSION_RUN);
    assert_eq!(
        request_body(&command)["command"],
        json!("/compact keep tests")
    );
    let spaced = parse(&words(&["command", "abc", "/model sonnet"])).unwrap();
    assert_eq!(request_body(&spaced)["command"], json!("/model sonnet"));
    let pick = parse(&words(&["dialog", "abc", "--pick", "2", "--dry-run"])).unwrap();
    assert_eq!(pick.verb.op(), ops::SESSION_PICK);
    assert_eq!(
        request_body(&pick),
        json!({"id": "abc", "number": 2, "consent": {}, "dryRun": true})
    );
    let dismiss = parse(&words(&["dialog", "abc", "--dismiss"])).unwrap();
    assert_eq!(request_body(&dismiss)["dismiss"], json!(true));
    let on = parse(&words(&["chrome", "abc", "on"])).unwrap();
    assert_eq!(on.verb.op(), ops::SESSION_BROWSE);
    assert_eq!(request_body(&on)["on"], json!(true));
    let off = parse(&words(&["chrome", "abc", "off"])).unwrap();
    assert_eq!(request_body(&off)["on"], json!(false));
    let option = parse(&words(&["options", "abc", "mode", "accept-edits"])).unwrap();
    assert_eq!(option.verb.op(), ops::SESSION_TUNE);
    assert_eq!(
        request_body(&option),
        json!({"id": "abc", "option": "mode", "value": "accept-edits", "consent": {}})
    );
}

#[test]
fn the_new_verbs_refuse_a_missing_or_clashing_part() {
    for args in [
        &["answer", "abc"][..],
        &["answer", "abc", "--answers", "not json"],
        &["dialog", "abc"],
        &["dialog", "abc", "--pick", "10"],
        &["dialog", "abc", "--pick", "1", "--dismiss"],
        &["chrome", "abc", "maybe"],
        &["chrome", "abc"],
        &["options", "abc", "mode"],
        &["command", "abc"],
        &["resume"],
        &["resume", "a", "b"],
        &["resume", "abc", "--model", "sonnet"],
        &["resume", "abc", "--", "extra"],
    ] {
        assert!(parse(&words(args)).is_err(), "{args:?}");
    }
}

#[test]
fn resume_names_a_session_and_its_flags() {
    let a = parse(&words(&[
        "resume",
        "abc",
        "--reason",
        "continue work",
        "--no-wait",
        "--timeout",
        "30",
        "--expires-in",
        "120",
        "--key",
        "k1",
        "--retry-of",
        "c0",
        "--dry-run",
        "--json",
    ]))
    .unwrap();
    assert_eq!(a.verb, Verb::Resume);
    assert_eq!(a.verb.op(), ops::SESSION_RESUME);
    assert!(a.json && !a.flags.wait && a.flags.dry_run);
    assert_eq!(a.flags.timeout, Duration::from_secs(30));
    assert_eq!(
        request_body(&a),
        json!({
            "id": "abc",
            "dryRun": true,
            "consent": {
                "reason": "continue work",
                "expiresInMs": 120_000,
                "key": "k1",
                "retryOf": "c0",
            },
        })
    );
    assert_eq!(
        done_line(Verb::Resume, true, "s1"),
        "Resumed the session s1."
    );
    assert_eq!(
        done_line(Verb::Resume, false, "j1"),
        "Resumed the session j1 (the user said yes)."
    );
}
