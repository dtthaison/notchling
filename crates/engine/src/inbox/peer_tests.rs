use super::*;

fn offers(ids: &[ActionId]) -> Vec<Action> {
    ids.iter()
        .map(|id| Action {
            id: *id,
            label: String::new(),
            primary: false,
        })
        .collect()
}

#[test]
fn a_consent_that_offers_no_allow_never_gets_one() {
    let got = peer_alert_actions(Kind::Consent, &offers(&[ActionId::Deny]));
    assert_eq!(got, vec![AlertAction::Deny, AlertAction::Open]);
}

#[test]
fn a_peer_consent_is_never_allowed_from_a_banner() {
    let got = peer_alert_actions(Kind::Consent, &offers(&[ActionId::Allow, ActionId::Deny]));
    assert!(!got.contains(&AlertAction::Allow));
    assert_eq!(got.len(), 3);
}

#[test]
fn a_secret_approval_and_other_kinds_get_only_what_their_kind_allows() {
    let approval = peer_alert_actions(
        Kind::SecretApproval,
        &offers(&[ActionId::Allow, ActionId::AllowOnce, ActionId::Deny]),
    );
    assert_eq!(
        approval,
        vec![AlertAction::Deny, AlertAction::Open, AlertAction::Change]
    );
    assert!(peer_alert_actions(Kind::Notice, &offers(&[ActionId::Done])).is_empty());
    assert!(peer_alert_actions(Kind::NodeJoin, &[]).is_empty());
}
