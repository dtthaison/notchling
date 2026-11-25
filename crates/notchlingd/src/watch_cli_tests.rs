use crate::contract::{self, Failure};
use crate::watch_cli::{parse, reopen, run};
use std::time::{Duration, Instant};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn quick(_: usize) -> Duration {
    Duration::from_millis(1)
}

#[test]
fn an_empty_until_is_a_usage_error() {
    let err = parse(&args(&["--until", ""])).unwrap_err();
    assert!(err.starts_with("--until needs a state"), "{err}");
}

#[test]
fn until_needs_a_topic_that_carries_a_state() {
    let err = parse(&args(&["--topic", "settings.switch", "--until", "applied"])).unwrap_err();
    assert!(err.contains("--until"), "{err}");
    let err = parse(&args(&[
        "--topic",
        "settings.changed",
        "--topic",
        "settings.switch",
        "--until",
        "applied",
    ]))
    .unwrap_err();
    assert!(err.contains("--until"), "{err}");
    for ok in [
        args(&["--until", "applied"]),
        args(&["--topic", "mine.consents", "--until", "applied"]),
        args(&["--topic", "mine.asks", "--until", "answered"]),
        args(&[
            "--topic",
            "settings.switch",
            "--topic",
            "mine.consents",
            "--until",
            "applied",
        ]),
        args(&["--topic", "settings.switch"]),
    ] {
        assert!(parse(&ok).is_ok(), "{ok:?}");
    }
}

#[test]
fn a_huge_timeout_is_a_usage_error_not_a_forever_wait() {
    let huge = args(&[
        "--timeout",
        "18446744073709551615",
        "--socket",
        "/nonexistent/notchling.sock",
    ]);
    assert!(parse(&huge).is_ok());
    assert_eq!(run(&huge), contract::BAD_REQUEST);
}

#[test]
fn a_reconnect_that_the_daemon_refuses_stops_with_its_error() {
    let mut calls = 0;
    let mut step = 0;
    let out = reopen(
        || {
            calls += 1;
            Err::<u32, _>(Failure::new(contract::REFUSED, "no_session", "no session"))
        },
        None,
        &mut step,
        quick,
    );
    let failure = out.unwrap_err();
    assert_eq!((failure.exit, failure.reason.as_str()), (4, "no_session"));
    assert_eq!(calls, 1);
}

#[test]
fn a_reconnect_keeps_trying_while_the_daemon_is_not_running() {
    let mut calls = 0;
    let mut step = 0;
    let out = reopen(
        || {
            calls += 1;
            if calls < 3 {
                Err(Failure::not_running())
            } else {
                Ok(7u32)
            }
        },
        None,
        &mut step,
        quick,
    );
    assert_eq!(out.unwrap(), Some(7));
    assert_eq!((calls, step), (3, 3));
}

#[test]
fn a_reconnect_does_not_open_after_the_timeout() {
    let mut calls = 0;
    let mut step = 0;
    let end = Instant::now() + Duration::from_millis(60);
    let out = reopen(
        || {
            calls += 1;
            Err::<u32, _>(Failure::not_running())
        },
        Some(end),
        &mut step,
        |_| Duration::from_secs(30),
    );
    assert_eq!(out.unwrap(), None);
    assert_eq!(calls, 0);
    assert!(Instant::now() >= end);
}

#[test]
fn a_reconnect_after_the_timeout_never_starts() {
    let mut calls = 0;
    let mut step = 0;
    let out = reopen(
        || {
            calls += 1;
            Ok(1u32)
        },
        Some(Instant::now() - Duration::from_millis(1)),
        &mut step,
        quick,
    );
    assert_eq!(out.unwrap(), None);
    assert_eq!(calls, 0);
}
