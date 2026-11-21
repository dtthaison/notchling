use super::*;
use notchling_local_api::{ApiError, ClientError, ErrorCode};
use serde_json::json;

fn api(code: ErrorCode, data: Option<Value>) -> ClientError {
    let mut e = ApiError::new(code, "fixed text");
    e.data = data;
    ClientError::Api(e)
}

#[test]
fn the_table_has_every_code_once_and_every_reason_maps_to_one() {
    let codes: Vec<i32> = EXIT_CODES.iter().map(|(c, _, _)| *c).collect();
    assert_eq!(codes, vec![0, 1, 2, 3, 4, 5, 6, 7, 124, 125]);
    for reason in REASONS {
        assert!(exit_of_reason(reason).is_some(), "{reason}");
    }
    assert_eq!(exit_of_reason("nothing"), None);
    assert_eq!(name_of(EXPIRED), "expired");
    assert_eq!(name_of(99), "failed");
}

#[test]
fn a_reason_from_the_daemon_sets_the_exit_and_the_fields() {
    let data = json!({
        "reason": "switch_off", "switch": "cluster",
        "ask": {"command": "notchling switch on cluster", "kind": "switch"},
        "never": false, "phase": 4, "retryAfterMs": 500,
    });
    let f = Failure::from_client(
        Some("cluster.dispatch"),
        api(ErrorCode::Forbidden, Some(data)),
    );
    assert_eq!(
        (f.exit, f.code.as_str(), f.reason.as_str()),
        (4, "forbidden", "switch_off")
    );
    assert_eq!(f.switch.as_deref(), Some("cluster"));
    assert_eq!(f.ask.as_ref().unwrap()["kind"], "switch");
    assert_eq!((f.phase, f.retry_after_ms), (Some(4), Some(500)));
    let v = f.to_value();
    assert_eq!(v["error"]["exit"], 4);
    assert_eq!(v["error"]["op"], "cluster.dispatch");
    assert_eq!(v["error"]["never"], false);
}

#[test]
fn json_is_a_flag_only_where_a_flag_can_stand() {
    let a = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    assert!(wants_json(&a(&["set", "k", "v", "--json"])));
    assert!(!wants_json(&a(&["set", "k", "v", "--reason", "--json"])));
    assert!(!wants_json(&a(&["gh", "--note", "--json", "--how", "x"])));
    assert!(wants_json(&a(&["gh", "--note", "x", "--json"])));
    assert!(!wants_json(&a(&["gh", "--socket", "--json"])));
    assert!(!wants_json(&a(&["gh", "--", "make", "--json"])));
    assert!(wants_json(&a(&["--json", "gh", "--", "make"])));
    assert!(wants_json(&a(&[
        "codex", "--model", "m", "--json", "--", "p"
    ])));
    assert!(!wants_json(&a(&["codex", "--model", "--json", "--", "p"])));
}

#[test]
fn a_reason_the_list_does_not_know_is_ignored() {
    let data = json!({"reason": "made_up"});
    let f = Failure::from_client(None, api(ErrorCode::Forbidden, Some(data)));
    assert_eq!((f.exit, f.reason.as_str()), (4, "other"));
}

#[test]
fn each_wire_code_without_a_reason_has_a_default() {
    let of = |code| {
        let f = Failure::from_client(None, api(code, None));
        (f.exit, f.reason)
    };
    assert_eq!(of(ErrorCode::BadRequest), (2, "bad_value".to_string()));
    assert_eq!(of(ErrorCode::Forbidden), (4, "other".to_string()));
    assert_eq!(of(ErrorCode::NeedsApproval), (4, "no_app".to_string()));
    assert_eq!(of(ErrorCode::Failed), (1, "failed".to_string()));
    assert_eq!(of(ErrorCode::Internal), (1, "internal".to_string()));
    assert_eq!(of(ErrorCode::UnknownOp), (3, "old_daemon".to_string()));
}

#[test]
fn the_persons_answer_and_the_clock_have_their_own_codes() {
    let denied = Failure::from_client(
        None,
        api(ErrorCode::NeedsApproval, Some(json!({"reason": "denied"}))),
    );
    assert_eq!((denied.exit, denied.code.as_str()), (5, "denied"));
    let late = Failure::from_client(
        None,
        api(ErrorCode::Failed, Some(json!({"reason": "expired"}))),
    );
    assert_eq!((late.exit, late.code.as_str()), (124, "expired"));
    let busy = Failure::from_client(
        None,
        api(
            ErrorCode::Failed,
            Some(json!({"reason": "busy", "retryAfterMs": 9})),
        ),
    );
    assert_eq!(
        (busy.exit, busy.code.as_str(), busy.retry_after_ms),
        (7, "busy", Some(9))
    );
}

#[test]
fn trouble_reaching_the_daemon_is_exit_three() {
    let io = || std::io::Error::from(std::io::ErrorKind::NotFound);
    for (error, reason) in [
        (ClientError::NotRunning(io()), "not_running"),
        (ClientError::Unsupported, "not_running"),
        (ClientError::Timeout, "timeout"),
        (ClientError::UntrustedServer("x".into()), "untrusted_daemon"),
    ] {
        let f = Failure::from_client(None, error);
        assert_eq!((f.exit, f.reason.as_str()), (3, reason));
    }
    assert_eq!(
        Failure::from_client(None, ClientError::Protocol("q".into())).exit,
        1
    );
}

#[test]
fn a_lost_daemon_is_retried_with_a_growing_pause_and_nothing_else_is() {
    assert!(lost(&Failure::not_running()));
    for failure in [
        Failure::failed("x"),
        Failure::new(NOT_RUNNING, "old_daemon", OLD_DAEMON),
        Failure::new(NOT_RUNNING, "untrusted_daemon", "x"),
        Failure::new(DENIED, "denied", "x"),
    ] {
        assert!(!lost(&failure), "{}", failure.reason);
    }
    let pauses: Vec<u64> = (0..6).map(pause_ms).collect();
    assert_eq!(pauses, [1_000, 2_000, 5_000, 10_000, 10_000, 10_000]);
}

#[test]
fn the_message_never_quotes_a_value_and_the_line_ends_with_the_name() {
    let f = Failure::from_client(None, ClientError::Protocol("secret-token".into()));
    assert!(!f.to_value().to_string().contains("secret-token"));
    let u = Failure::usage("settings get needs a key.\nUsage: x");
    assert!(u.line().ends_with("Usage: x (bad_request)"));
    assert!(!u.to_value().to_string().contains("settings get"));
}

#[test]
fn json_goes_to_stdout_only_and_text_to_stderr_only() {
    let f = Failure::not_running();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(f.emit(true, &mut out, &mut err), 3);
    assert!(err.is_empty());
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["error"]["code"], "not_running");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(f.emit(false, &mut out, &mut err), 3);
    assert!(out.is_empty());
    assert!(String::from_utf8(err)
        .unwrap()
        .ends_with("then try again. (not_running)\n"));
}

#[test]
fn json_is_read_only_before_the_double_dash() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(wants_json(&args(&["--json", "gh", "--", "make"])));
    assert!(!wants_json(&args(&["gh", "--", "make", "--json"])));
    assert!(!wants_json(&args(&[])));
}
