use super::job_kinds_tests::row_of;
use super::*;
use crate::inbox::AlertAction::{self, *};
use serde_json::json;

fn alerts(kind: &str, target: &str) -> Vec<AlertAction> {
    let mut row = row_of(kind, AnswerOn::Mac, json!({}));
    row.target = target.to_string();
    alert_actions(&row)
}

#[test]
fn the_daemon_kinds_may_be_allowed_from_a_banner() {
    for kind in [SETTING, SESSION_STOP, JOB_STOP] {
        assert_eq!(alerts(kind, "x"), vec![Allow, Deny, Change], "{kind}");
    }
}

#[test]
fn a_switch_is_allowed_from_a_banner_only_when_it_is_not_sensitive() {
    for target in ["accounts", "install", "machines"] {
        assert_eq!(
            alerts(SWITCH, target),
            vec![Allow, Deny, Change],
            "{target}"
        );
    }
    for target in ["acting", "vault", "bridge", "cluster"] {
        assert_eq!(alerts(SWITCH, target), vec![Deny, Open, Change], "{target}");
    }
}

#[test]
fn the_kinds_that_need_the_card_offer_deny_open_and_change() {
    for kind in [
        SESSION,
        DISPATCH,
        JOB,
        MODEL,
        UPDATE,
        ACCOUNT,
        MACHINE,
        INSTALL,
        COST_PROFILE,
        CLUSTER,
    ] {
        assert_eq!(alerts(kind, "x"), vec![Deny, Open, Change], "{kind}");
    }
}

#[test]
fn a_secret_request_offers_the_form_and_cancel() {
    assert_eq!(
        alerts(SECRET_REQUEST, "npm"),
        vec![OpenForm, Cancel, Change]
    );
}

#[test]
fn an_unverified_caller_gets_only_deny_and_open_whatever_it_asks() {
    for kind in [SETTING, SWITCH, SECRET_REQUEST, SESSION, CLUSTER] {
        let mut row = row_of(kind, AnswerOn::Mac, json!({}));
        row.verified = false;
        assert_eq!(alert_actions(&row), vec![Deny, Open], "{kind}");
    }
}

#[test]
fn an_unknown_kind_offers_nothing() {
    assert_eq!(alerts("mystery", "x"), Vec::<AlertAction>::new());
}

#[test]
fn the_item_carries_the_alert_actions_and_the_session_as_its_place() {
    let mut row = row_of(SETTING, AnswerOn::MacOrPhone, json!({}));
    row.session = Some("s1".to_string());
    let made = item(&row, 5);
    assert_eq!(made.alert_actions, vec![Allow, Deny, Change]);
    assert_eq!(made.place.and_then(|p| p.session), Some("s1".to_string()));
}

#[test]
fn the_item_names_the_consent_kind_so_the_notch_can_derive_allow_from_it() {
    for kind in [SETTING, SWITCH, SESSION, CLUSTER] {
        let row = row_of(kind, AnswerOn::Mac, json!({}));
        let made = item(&row, 5);
        let open = made.open.unwrap();
        assert_eq!(open["kind"], kind, "{kind}");
        assert_eq!(open["consent"], row.id, "{kind}");
    }
}
