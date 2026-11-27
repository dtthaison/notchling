use crate::acting_ops_tests::{app, code, row, setup, Setup, PROMPT};
use notchling_engine::reply::exec::Failure;
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};

fn fixture(name: &str) -> String {
    format!(
        "{}/../engine/tests/fixtures/notchling/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn list(transcript: &str) -> Value {
    json!([row(
        "job-a",
        json!({"job": "abcd1234"}),
        "working",
        Some(transcript)
    )])
}

fn question() -> Value {
    json!({"items": [{"question": "Which database?", "options": ["SQLite", "Postgres"], "multiSelect": false}]})
}

fn answer(s: &Setup) -> String {
    let (c, message) = code(s.ops.call(
        &app(),
        ops::SESSION_ANSWER,
        &json!({"id": "job-a", "answers": [[1]], "question": question()}),
    ));
    assert_eq!(c, ErrorCode::Failed);
    message
}

#[test]
fn an_answer_to_a_signed_out_session_says_the_login_expired() {
    let s = setup(true, PROMPT, list(&fixture("login-expired.jsonl")));
    let message = answer(&s);
    assert!(message.contains("/login"), "{message}");
    assert!(!message.contains("isn't open"), "{message}");
    assert_eq!(message, Failure::LoginNeeded.to_string());
    assert!(s.writes.lock().unwrap().is_empty(), "nothing typed");
}

#[test]
fn an_answer_to_a_closed_question_still_says_it_is_closed_when_the_login_is_fine() {
    let s = setup(
        true,
        PROMPT,
        list(&fixture("login-expired-then-answered.jsonl")),
    );
    let message = answer(&s);
    assert_eq!(message, Failure::QuestionGone.to_string());
}

#[test]
fn a_session_without_a_transcript_keeps_the_closed_question_words() {
    let s = setup(true, PROMPT, list("/nonexistent/none.jsonl"));
    assert_eq!(answer(&s), Failure::QuestionGone.to_string());
}
