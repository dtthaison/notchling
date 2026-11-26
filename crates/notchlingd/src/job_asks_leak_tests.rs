use super::failure_tests::{dispatch_failed, failing_with, Failing};
use super::*;
use crate::consent_testkit::agent_caller;
use std::time::Duration;

const LEAKS: [&str; 4] = [
    "person@example.com",
    "/Users/sondoan",
    "https://example.com",
    "Connection refused",
];

fn leaky() -> ApiError {
    ApiError::new(
        ErrorCode::Failed,
        "ssh build: person@example.com refused at /Users/sondoan/work; fetch https://example.com/x failed: Connection refused",
    )
}

fn failing(tag: &str, error: ApiError) -> Failing {
    failing_with(tag, error, Arc::new(JobBook::in_memory()))
}

fn shown_to(f: &Failing, done: &Consent) -> String {
    let mut text = String::new();
    for agent in [true, false] {
        let view = f.consent.project(done, agent);
        let failure = crate::consent_cli::failure_of(&view, ops::CONSENT_GET);
        text.push_str(&format!(
            "{view} {} {:?} {:?}",
            failure.message, failure.human, failure.request
        ));
    }
    text
}

fn assert_clean(text: &str) {
    for leak in LEAKS {
        assert!(!text.contains(leak), "{leak} leaked: {text}");
    }
}

fn card_for(f: &Failing, op: &str, id: &str) -> Consent {
    let reply = f
        .asks
        .call(&agent_caller(), op, &json!({"id": id}))
        .unwrap();
    let card = reply["request"]["id"].as_str().unwrap().to_string();
    let said = f.consent.decide(&card, true, None, "mac", None).unwrap();
    assert_eq!(said["answered"], json!(true));
    f.consent.wait_for(&card, Duration::from_secs(5)).unwrap()
}

#[test]
fn a_dispatch_that_fails_with_raw_text_leaves_only_a_code_and_known_words() {
    let f = failing("leak-dispatch", leaky());
    let done = dispatch_failed(&f, "k-leak");
    assert_eq!(done.state.as_str(), "failed");
    let view = f.consent.project(&done, true);
    assert_eq!(view["error"]["code"], "failed", "{view}");
    assert!(view["error"]["message"]
        .as_str()
        .is_some_and(|m| !m.is_empty()));
    assert_clean(&shown_to(&f, &done));
}

#[test]
fn a_named_reason_keeps_its_code_and_gets_known_words_not_the_workers_text() {
    let f = failing(
        "leak-reason",
        leaky()
            .with_reason("worker_claude_signed_out")
            .with_field("wire", json!("worker_claude_signed_out")),
    );
    let done = dispatch_failed(&f, "k-leak-reason");
    let view = f.consent.project(&done, true);
    assert_eq!(view["error"]["code"], "worker_claude_signed_out", "{view}");
    assert!(
        view["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("signed in")),
        "{view}"
    );
    assert_clean(&shown_to(&f, &done));
}

#[test]
fn a_reason_that_is_not_a_plain_code_is_not_stored() {
    let f = failing(
        "leak-code",
        leaky().with_reason("person@example.com /Users/sondoan"),
    );
    let done = dispatch_failed(&f, "k-leak-code");
    let view = f.consent.project(&done, true);
    assert_eq!(view["error"]["code"], "failed", "{view}");
    assert_clean(&shown_to(&f, &done));
}

#[test]
fn a_tail_a_return_and_a_stop_that_fail_leave_no_raw_text() {
    for (op, tag) in [
        (ops::JOBS_TAIL, "leak-tail"),
        (ops::JOBS_RETURN, "leak-return"),
        (ops::JOBS_STOP, "leak-stop"),
    ] {
        let f = failing(tag, leaky());
        let done = card_for(&f, op, "j-own");
        assert_eq!(done.state.as_str(), "failed", "{op}");
        let view = f.consent.project(&done, true);
        assert_eq!(view["error"]["code"], "failed", "{op}: {view}");
        assert_clean(&shown_to(&f, &done));
    }
}
