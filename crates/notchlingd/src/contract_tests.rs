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
    for mistake in [
        "usage",
        "bad_value",
        "unknown_topic",
        "unknown_key",
        "key_reused",
        "unknown_tool",
        "unknown_pool",
        "ambiguous_pool",
    ] {
        assert_eq!(exit_of_reason(mistake), Some(BAD_REQUEST), "{mistake}");
    }
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
    for flag in ["--kind", "--tool", "--pool", "--since"] {
        assert!(!wants_json(&a(&["search", flag, "--json"])), "{flag}");
        assert!(wants_json(&a(&["search", flag, "x", "--json"])), "{flag}");
    }
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

struct Scripted {
    sends: Vec<Result<Value, ClientError>>,
    renews: Vec<Result<(), ClientError>>,
    sent: Vec<(Value, Duration)>,
    renew_takes: Duration,
    renewed_within: Vec<Duration>,
}

impl Link for Scripted {
    fn send(&mut self, _op: &str, args: Value, timeout: Duration) -> Result<Value, ClientError> {
        self.sent.push((args, timeout));
        if self.sends.is_empty() {
            Err(ClientError::Closed)
        } else {
            self.sends.remove(0)
        }
    }

    fn renew(&mut self, within: Duration) -> Result<(), ClientError> {
        self.renewed_within.push(within);
        std::thread::sleep(self.renew_takes);
        if self.renews.is_empty() {
            Err(ClientError::Closed)
        } else {
            self.renews.remove(0)
        }
    }
}

fn scripted(
    sends: Vec<Result<Value, ClientError>>,
    renews: Vec<Result<(), ClientError>>,
) -> Scripted {
    Scripted {
        sends,
        renews,
        sent: Vec::new(),
        renew_takes: Duration::ZERO,
        renewed_within: Vec::new(),
    }
}

fn left_ms(left: Duration) -> (Value, Duration) {
    let ms = u64::try_from(left.as_millis())
        .unwrap_or(u64::MAX)
        .min(50_000);
    (json!({"waitMs": ms}), Duration::from_millis(ms + 5))
}

fn tiny(_: usize) -> Duration {
    Duration::from_millis(5)
}

#[test]
fn a_reconnect_refused_for_good_is_passed_on_at_once() {
    let mut link = scripted(
        vec![Err(ClientError::Closed)],
        vec![
            Err(ClientError::NotRunning(std::io::ErrorKind::NotFound.into())),
            Err(ClientError::UntrustedServer("x".into())),
        ],
    );
    let end = Instant::now() + Duration::from_secs(30);
    let got = through_restart(&mut link, "consent.get", &left_ms, end, &tiny);
    assert!(
        matches!(got, Err(ClientError::UntrustedServer(_))),
        "{got:?}"
    );
    assert_eq!(link.sent.len(), 1);
}

#[test]
fn a_dead_socket_write_is_retried_and_the_wait_shrinks_to_what_is_left() {
    let mut link = scripted(
        vec![
            Err(ClientError::Io(std::io::ErrorKind::BrokenPipe.into())),
            Ok(json!({"state": "waiting"})),
        ],
        vec![Err(ClientError::Closed), Ok(())],
    );
    let end = Instant::now() + Duration::from_secs(20);
    let got = through_restart(&mut link, "consent.get", &left_ms, end, &tiny).unwrap();
    assert_eq!(got["state"], "waiting");
    let first = link.sent[0].0["waitMs"].as_u64().unwrap();
    let second = link.sent[1].0["waitMs"].as_u64().unwrap();
    assert!(second < first && second <= 20_000, "{first} {second}");
    assert_eq!(link.sent[1].1, Duration::from_millis(second + 5));
}

#[test]
fn no_reconnect_or_retry_starts_after_the_deadline() {
    let mut link = scripted(vec![], vec![]);
    let end = Instant::now() + Duration::from_millis(40);
    let started = Instant::now();
    let got = through_restart(&mut link, "consent.get", &left_ms, end, &|_| {
        Duration::from_millis(30)
    });
    assert!(matches!(got, Err(ClientError::Closed)), "{got:?}");
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(link.sent.len() <= 2, "{}", link.sent.len());
    assert!(link
        .sent
        .iter()
        .all(|(a, _)| a["waitMs"].as_u64().unwrap() <= 40));
}

#[test]
fn only_a_lost_link_is_transient() {
    for e in [
        ClientError::Closed,
        ClientError::NotRunning(std::io::ErrorKind::NotFound.into()),
        ClientError::Io(std::io::ErrorKind::BrokenPipe.into()),
        ClientError::Io(std::io::ErrorKind::ConnectionReset.into()),
    ] {
        assert!(transient(&e), "{e}");
    }
    for e in [
        ClientError::UntrustedServer("x".into()),
        ClientError::Timeout,
        ClientError::Unsupported,
        ClientError::Io(std::io::ErrorKind::PermissionDenied.into()),
        api(ErrorCode::Failed, None),
    ] {
        assert!(!transient(&e), "{e}");
    }
}

#[test]
fn a_reconnect_that_ends_past_the_deadline_sends_nothing_more() {
    let mut link = scripted(
        vec![Err(ClientError::Closed), Ok(json!({"state": "waiting"}))],
        vec![Ok(())],
    );
    link.renew_takes = Duration::from_millis(80);
    let end = Instant::now() + Duration::from_millis(40);
    let got = through_restart(&mut link, "consent.get", &left_ms, end, &tiny);
    assert!(matches!(got, Err(ClientError::Closed)), "{got:?}");
    assert_eq!(link.sent.len(), 1);
    assert!(link
        .renewed_within
        .iter()
        .all(|w| *w <= Duration::from_millis(40)));
}

#[test]
fn a_lost_link_at_the_deadline_is_a_lost_daemon_whatever_its_kind() {
    for e in [
        ClientError::Closed,
        ClientError::Io(std::io::ErrorKind::BrokenPipe.into()),
        ClientError::Io(std::io::ErrorKind::UnexpectedEof.into()),
    ] {
        let failure = restart_failure("consent.get", e);
        assert!(lost(&failure), "{}", failure.reason);
        assert_eq!(failure.op.as_deref(), Some("consent.get"));
    }
    assert!(!lost(&restart_failure(
        "consent.get",
        ClientError::Io(std::io::ErrorKind::PermissionDenied.into())
    )));
}
