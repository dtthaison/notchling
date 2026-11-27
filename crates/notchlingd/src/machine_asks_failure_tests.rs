use super::machine_asks_rig::{machine, on_tailscale, reason, rig, Rig, LEAKS};
use crate::cited::Cited;
use crate::consent_testkit::app_caller;
use notchling_local_api::{ops, ApiError, ErrorCode};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::time::Duration;

fn broken(tag: &str) -> Rig {
    let r = rig(tag, vec![machine("mine", None)]);
    r.read(ops::MACHINES_CHECK, json!({"machine": "mine"}))
        .unwrap();
    r.breaks.store(true, Ordering::SeqCst);
    r
}

#[test]
fn a_fix_that_fails_names_the_step_and_why_to_the_caller() {
    let r = broken("fix-fail-caller");
    let id = r.card("fix", json!({"machine": "mine", "step": "plain"}));
    let err = r.apply(&id).unwrap_err();
    assert_eq!(reason(&err), "step_failed");
    assert!(err.message.contains("A step"), "{}", err.message);
    assert!(
        err.message.contains("Connection refused"),
        "{}",
        err.message
    );
    assert_eq!(err.data.as_ref().unwrap()["step"], "plain");
}

fn shown_to(view: &Value) -> String {
    let failure = crate::consent_cli::failure_of(view, ops::CONSENT_GET);
    format!(
        "{view} {} {:?} {:?}",
        failure.message, failure.human, failure.request
    )
}

fn assert_clean(text: &str) {
    for leak in LEAKS {
        assert!(!text.contains(leak), "{leak} leaked: {text}");
    }
}

#[test]
fn a_fix_that_fails_leaves_a_code_and_known_words_on_the_request() {
    let r = broken("fix-fail-card");
    let id = r.card("fix", json!({"machine": "mine", "step": "plain"}));
    let _ = r.apply(&id);
    let row = r.consent.wait_for(&id, Duration::from_secs(5)).unwrap();
    assert_eq!(row.state.as_str(), "failed");
    let view = r.consent.project(&row, true);
    assert_eq!(view["error"]["code"], "step_failed", "{view}");
    assert_eq!(view["error"]["step"], "plain", "{view}");
    assert!(view["error"]["message"]
        .as_str()
        .is_some_and(|m| !m.is_empty()));
    assert_clean(&shown_to(&view));
    assert_clean(&shown_to(&r.consent.project(&row, false)));
}

#[test]
fn a_network_switch_that_fails_leaves_no_tailscale_text_on_the_request() {
    let r = rig(
        "switch-fail-card",
        vec![machine("mine", None), on_tailscale(machine("tsbox", None))],
    );
    let id = r.card("network", json!({"tailnet": "work.example"}));
    let row = r.consent.get_row(&id).unwrap();
    let mut args = row.change["apply"]["args"].clone();
    args["consentId"] = json!(id);
    let broken = |_: &Value| {
        Err(ApiError::new(
            ErrorCode::Failed,
            "tailscale: person@example.com is signed out at /Users/sondoan/.config; see https://example.com/login; id_ed25519; Connection refused",
        ))
    };
    r.asks
        .apply(&app_caller(), ops::TAILSCALE_SWITCH, &args, &broken)
        .unwrap_err();
    let row = r.consent.wait_for(&id, Duration::from_secs(5)).unwrap();
    assert_eq!(row.state.as_str(), "failed");
    let view = r.consent.project(&row, true);
    assert_eq!(view["error"]["code"], "failed", "{view}");
    assert_clean(&shown_to(&view));
    assert_clean(&shown_to(&r.consent.project(&row, false)));
}
