use super::tests::{agent, app, ask, ended, setup};
use super::*;
use crate::consent_testkit::call_op;
use crate::contract;
use std::time::Duration;

fn opened_request(tag: &str) -> (super::tests::Setup, String) {
    let s = setup(tag, true);
    let (reply, _) = ask(
        &s,
        json!({"name": "emqx-dashboard-dev-admin", "kind": "login"}),
    );
    let id = reply["request"]["id"].as_str().unwrap().to_string();
    let opened = call_op(
        &s.consent,
        &app(),
        ops::CONSENT_OPENED,
        json!({"consentId": id}),
    );
    assert_eq!(opened["state"], "opened");
    (s, id)
}

#[test]
fn cancelling_the_opened_form_ends_the_row_as_denied_by_the_mac() {
    let (s, id) = opened_request("form-cancel-row");
    let denied = s
        .ops
        .call(&app(), ops::VAULT_REQUEST_CANCEL, &json!({"id": id}))
        .unwrap();
    assert_eq!(denied["answered"], true);
    let row = ended(&s, &id);
    assert_eq!(row["state"], "denied");
    assert_eq!(row["by"], "mac");
}

#[test]
fn a_waiting_cli_sees_the_cancel_and_exits_5_with_a_clear_message() {
    let (s, id) = opened_request("form-cancel-wait");
    let consent = s.consent.clone();
    let waiter = {
        let id = id.clone();
        std::thread::spawn(move || {
            let end = std::time::Instant::now() + Duration::from_secs(20);
            let first = call_op(&consent, &agent(), ops::CONSENT_GET, json!({"id": id}));
            assert_eq!(first["state"], "opened");
            contract::wait_while_pending(first, true, end, || {
                Ok(call_op(
                    &consent,
                    &agent(),
                    ops::CONSENT_GET,
                    json!({"id": id, "waitMs": 2000}),
                ))
            })
        })
    };
    std::thread::sleep(Duration::from_millis(100));
    s.ops
        .call(&app(), ops::VAULT_REQUEST_CANCEL, &json!({"id": id}))
        .unwrap();
    let seen = waiter.join().unwrap().unwrap();
    assert_eq!(seen["state"], "denied");
    let failure = crate::vault_cli::ended(&seen, "emqx-dashboard-dev-admin");
    assert_eq!(failure.exit, 5);
    assert_eq!(failure.reason, "denied");
    assert!(
        failure
            .message
            .contains("declined to save emqx-dashboard-dev-admin"),
        "{}",
        failure.message
    );
}

#[test]
fn cancelling_twice_or_after_the_save_changes_nothing() {
    let (s, id) = opened_request("form-cancel-twice");
    let first = s
        .ops
        .call(&app(), ops::VAULT_REQUEST_CANCEL, &json!({"id": id}))
        .unwrap();
    assert_eq!(first["answered"], true);
    let second = s
        .ops
        .call(&app(), ops::VAULT_REQUEST_CANCEL, &json!({"id": id}))
        .unwrap();
    assert_eq!(second["answered"], false);
    assert_eq!(ended(&s, &id)["state"], "denied");
}

#[test]
fn a_cancel_never_ends_a_consent_that_is_not_a_secret_request() {
    let s = setup("form-cancel-kind", true);
    let row = s
        .consent
        .open(
            &agent(),
            crate::consent_ops::Opening {
                kind: "setting",
                target: "showPet".into(),
                change: json!({}),
                facts: json!([]),
                request: json!({"op": "test", "target": "showPet"}),
                parts: None,
                reason: None,
                expires_in_ms: None,
                key: None,
                retry_of: None,
            },
        )
        .unwrap();
    let id = row.row().id.clone();
    let denied = s
        .ops
        .call(&app(), ops::VAULT_REQUEST_CANCEL, &json!({"id": id}))
        .unwrap();
    assert_eq!(denied["answered"], false);
    assert_eq!(s.consent.get_row(&id).unwrap().state.as_str(), "waiting");
}
