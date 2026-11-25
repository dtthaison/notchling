use super::*;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| (*w).to_string()).collect()
}

#[test]
fn consent_takes_an_id_cancel_takes_cancel_and_an_id_and_consents_takes_nothing() {
    let get = parse(
        &words(&["c1", "--wait", "--timeout", "30", "--json"]),
        false,
    )
    .unwrap();
    assert_eq!(get.sub, Sub::Get("c1".into()));
    assert!(get.wait && get.json);
    assert_eq!(get.timeout, Duration::from_secs(30));
    let cancel = parse(&words(&["cancel", "c1"]), false).unwrap();
    assert_eq!(cancel.sub, Sub::Cancel("c1".into()));
    assert_eq!(parse(&words(&[]), true).unwrap().sub, Sub::List);
    assert!(parse(&words(&[]), false).is_err());
    assert!(parse(&words(&["c1"]), true).is_err());
    assert!(parse(&words(&["c1", "--odd"]), false).is_err());
    assert!(parse(&words(&["c1", "--timeout", "soon"]), false).is_err());
}

#[test]
fn the_exit_follows_the_requests_state() {
    for (state, exit) in [
        ("applied", 0),
        ("waiting", 6),
        ("opened", 6),
        ("applying", 6),
        ("denied", 5),
        ("cancelled", 5),
        ("expired", 124),
        ("failed", 1),
        ("withdrawn", 1),
    ] {
        assert_eq!(exit_of(state), exit, "{state}");
    }
}

#[test]
fn a_request_that_still_waits_is_a_pending_error_that_carries_it() {
    let request = json!({"id": "c1", "kind": "setting", "target": "limitMode", "state": "waiting"});
    let failure = failure_of(&request, ops::CONSENT_GET);
    assert_eq!(failure.exit, contract::PENDING);
    assert_eq!(failure.reason, "needs_consent");
    assert_eq!(failure.request, Some(request));
    assert!(failure.line().ends_with("(pending)"), "{}", failure.line());
}
