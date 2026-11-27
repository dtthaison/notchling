mod common;

use common::*;
use notchling_engine::transcript::conversation::Parser;
use notchling_engine::transcript::login::{login_needed_in, login_needed_in_file};

const EXPIRED: &str = include_str!("fixtures/notchling/login-expired.jsonl");
const SIGNED_IN: &str = include_str!("fixtures/notchling/login-expired-then-signed-in.jsonl");
const ANSWERED: &str = include_str!("fixtures/notchling/login-expired-then-answered.jsonl");
const BLOCKED: &str = include_str!("fixtures/notchling/login-blocked-403.jsonl");
const LIMIT: &str = include_str!("fixtures/notchling/limit-hit.jsonl");

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

fn parsed(text: &str) -> bool {
    let mut parser = Parser::new();
    for line in text.lines() {
        parser.consume(line.as_bytes());
    }
    parser.conversation().login_needed
}

#[test]
fn an_expired_login_is_needed() {
    assert!(login_needed_in(lines(EXPIRED).into_iter()));
    assert!(parsed(EXPIRED));
}

#[test]
fn the_file_read_agrees_with_the_lines() {
    assert!(login_needed_in_file(&s(&notchling_fixture(
        "login-expired.jsonl"
    ))));
    assert!(!login_needed_in_file(&s(&notchling_fixture(
        "login-expired-then-answered.jsonl"
    ))));
    assert!(!login_needed_in_file("/nonexistent/none.jsonl"));
}

#[test]
fn a_later_successful_turn_clears_it() {
    assert!(!login_needed_in(lines(ANSWERED).into_iter()));
    assert!(!parsed(ANSWERED));
}

#[test]
fn signing_in_again_clears_it() {
    assert!(!login_needed_in(lines(SIGNED_IN).into_iter()));
    assert!(!parsed(SIGNED_IN));
}

#[test]
fn a_message_typed_after_the_error_does_not_clear_it() {
    let typed = r#"{"type":"user","uuid":"u-8","isSidechain":false,"message":{"role":"user","content":"use sqlite"}}"#;
    let mut all = lines(EXPIRED);
    all.push(typed);
    assert!(login_needed_in(all.into_iter()));
}

#[test]
fn an_error_that_is_not_about_the_login_is_not_needed() {
    assert!(!login_needed_in(lines(BLOCKED).into_iter()));
    assert!(!parsed(BLOCKED));
    assert!(!login_needed_in(lines(LIMIT).into_iter()));
    assert!(!parsed(LIMIT));
}

#[test]
fn another_error_after_it_clears_it() {
    let mut all = lines(EXPIRED);
    all.extend(lines(LIMIT));
    assert!(!login_needed_in(all.into_iter()));
}

#[test]
fn a_retry_that_says_401_is_needed_before_the_final_error_lands() {
    let retry = lines(EXPIRED)[2];
    assert!(login_needed_in([retry].into_iter()));
    let mut parser = Parser::new();
    parser.consume(retry.as_bytes());
    assert!(parser.conversation().login_needed);
}

#[test]
fn a_subagent_line_is_ignored() {
    let side = r#"{"type":"assistant","uuid":"s-1","isSidechain":true,"isApiErrorMessage":true,"error":"authentication_failed","apiErrorStatus":401,"message":{"role":"assistant","content":[{"type":"text","text":"Please run /login"}]}}"#;
    assert!(!login_needed_in([side].into_iter()));
    let mut parser = Parser::new();
    parser.consume(side.as_bytes());
    assert!(!parser.conversation().login_needed);
}

#[test]
fn the_message_the_user_sees_stays_in_the_conversation() {
    let mut parser = Parser::new();
    for line in EXPIRED.lines() {
        parser.consume(line.as_bytes());
    }
    let conversation = parser.conversation();
    assert!(conversation.pending_question.is_some());
    assert!(conversation.login_needed);
}

fn final_error(status: Option<u64>, text: &str) -> String {
    let status = status.map_or(String::new(), |s| format!(r#""apiErrorStatus":{s},"#));
    format!(
        r#"{{"type":"assistant","uuid":"e-1","isSidechain":false,"isApiErrorMessage":true,"error":"unknown",{status}"message":{{"role":"assistant","model":"<synthetic>","content":[{{"type":"text","text":"{text}"}}]}}}}"#
    )
}

fn retry_error(status: Option<u64>, text: &str) -> String {
    let status = status.map_or(String::new(), |s| format!(r#""status":{s},"#));
    format!(
        r#"{{"type":"system","uuid":"e-2","subtype":"api_error","isSidechain":false,"error":{{"message":"{text}",{status}"formatted":"{text}"}}}}"#
    )
}

fn needed(line: &str) -> bool {
    let mut parser = Parser::new();
    parser.consume(line.as_bytes());
    let parsed = parser.conversation().login_needed;
    assert_eq!(login_needed_in([line].into_iter()), parsed, "{line}");
    parsed
}

const AUTH_TEXTS: [&str; 3] = [
    "Please run /login",
    r#"API Error: {\"type\":\"authentication_error\"}"#,
    "OAuth token has expired",
];

#[test]
fn an_auth_word_under_another_status_is_not_needed() {
    for status in [403, 429, 500, 529] {
        for text in AUTH_TEXTS {
            assert!(!needed(&final_error(Some(status), text)), "{status} {text}");
            assert!(!needed(&retry_error(Some(status), text)), "{status} {text}");
        }
    }
}

#[test]
fn a_401_is_needed_whatever_it_says() {
    assert!(needed(&final_error(Some(401), "Something went wrong")));
    assert!(needed(&retry_error(Some(401), "Something went wrong")));
}

#[test]
fn an_auth_word_counts_only_when_there_is_no_status() {
    for text in AUTH_TEXTS {
        assert!(needed(&final_error(None, text)), "{text}");
        assert!(needed(&retry_error(None, text)), "{text}");
    }
    assert!(!needed(&final_error(None, "Something went wrong")));
    assert!(!needed(&retry_error(None, "Something went wrong")));
}

#[test]
fn a_403_with_an_auth_word_after_the_expiry_clears_it() {
    let mut all = lines(EXPIRED);
    let blocked = final_error(Some(403), "Please run /login");
    all.push(&blocked);
    assert!(!login_needed_in(all.into_iter()));
}
