use super::*;
use serde_json::json;
use std::time::{Duration, Instant};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn request(state: &str) -> Value {
    json!({"id": "c1", "state": state, "kind": "secret_request", "target": "emqx-dashboard-dev-admin"})
}

#[test]
fn a_denied_request_exits_5_and_says_the_user_declined_to_save_that_name() {
    let failure = ended(&request("denied"), "emqx-dashboard-dev-admin");
    assert_eq!(failure.exit, contract::DENIED);
    assert_eq!(failure.exit, 5);
    assert_eq!(failure.reason, "denied");
    assert!(
        failure
            .message
            .contains("declined to save emqx-dashboard-dev-admin"),
        "{}",
        failure.message
    );
    assert!(failure.human.as_deref().unwrap().contains("c1"));
}

#[test]
fn the_other_endings_keep_their_own_words_and_exits() {
    for (state, exit, reason) in [
        ("expired", 124, "expired"),
        ("withdrawn", 1, "withdrawn"),
        ("cancelled", 5, "cancelled"),
        ("failed", 1, "failed"),
    ] {
        let failure = ended(&request(state), "emqx-dashboard-dev-admin");
        assert_eq!(failure.exit, exit, "{state}");
        assert_eq!(failure.reason, reason, "{state}");
        assert!(!failure.message.contains("declined"), "{state}");
    }
}

#[test]
fn a_wait_that_sees_the_denial_stops_waiting() {
    let mut reads = 0;
    let end = Instant::now() + Duration::from_secs(30);
    let answered = contract::wait_while_pending(request("opened"), true, end, || {
        reads += 1;
        Ok(request(if reads < 3 { "opened" } else { "denied" }))
    })
    .unwrap();
    assert_eq!(reads, 3);
    assert_eq!(ended(&answered, "x").exit, 5);
}

#[test]
fn the_name_check_is_the_daemons_so_the_cli_sends_every_well_formed_name() {
    for name in ["dashboards", "api", "emqx-dashboard-dev-admin"] {
        assert!(
            parse_request(&args(&[name, "--kind", "login"])).is_ok(),
            "{name}"
        );
    }
}

#[test]
fn the_usage_is_said_when_no_name_is_given() {
    assert_eq!(
        parse_request(&args(&["--kind", "token"])).unwrap_err(),
        REQUEST_USAGE
    );
}

#[test]
fn a_noted_denial_prints_the_note_not_the_dont_ask_again_line() {
    let mut row = request("denied");
    row["note"] = json!("call it emqx-dashboard-prod");
    let failure = ended(&row, "emqx-dashboard-dev-admin");
    assert_eq!(failure.exit, 5);
    assert!(
        failure
            .message
            .contains("asked for a change: call it emqx-dashboard-prod"),
        "{}",
        failure.message
    );
    assert!(!failure.message.contains("Don't ask again"));
}
