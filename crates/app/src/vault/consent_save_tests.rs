use super::*;
use notchling_local_api::ErrorCode;

fn refused(message: &str, reason: &str) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, message).with_reason(reason)
}

fn ended() -> ApiError {
    refused(
        "That request is no longer waiting for your yes.",
        "needs_consent",
    )
}

fn drifted() -> ApiError {
    refused(CHANGED, "needs_consent")
}

#[test]
fn a_request_that_is_no_longer_waiting_is_expired_or_withdrawn_by_its_state() {
    assert_eq!(refusal(&ended(), Some("expired")), Some(Refusal::Expired));
    assert_eq!(
        refusal(&ended(), Some("cancelled")),
        Some(Refusal::Withdrawn)
    );
    assert_eq!(
        refusal(&ended(), Some("withdrawn")),
        Some(Refusal::Withdrawn)
    );
    assert_eq!(refusal(&ended(), Some("denied")), Some(Refusal::Withdrawn));
    assert_eq!(refusal(&ended(), None), Some(Refusal::Withdrawn));
}

#[test]
fn a_changed_entry_is_told_apart_from_a_request_that_ended_while_saving() {
    assert_eq!(refusal(&drifted(), Some("opened")), Some(Refusal::Changed));
    assert_eq!(refusal(&drifted(), None), Some(Refusal::Changed));
    assert_eq!(
        refusal(&drifted(), Some("expired")),
        Some(Refusal::Expired)
    );
    assert_eq!(
        refusal(&drifted(), Some("cancelled")),
        Some(Refusal::Withdrawn)
    );
}

#[test]
fn a_missing_request_is_withdrawn_and_the_vault_off_message_is_its_own_line() {
    let gone = ApiError::new(ErrorCode::Failed, "No such request.");
    assert_eq!(refusal(&gone, None), Some(Refusal::Withdrawn));
    let off = ApiError::new(ErrorCode::NeedsApproval, VAULT_OFF);
    assert_eq!(refusal(&off, None), Some(Refusal::VaultOff));
}

#[test]
fn any_other_error_keeps_the_daemons_words() {
    let other = ApiError::new(ErrorCode::Failed, "the Keychain refused");
    assert_eq!(refusal(&other, Some("opened")), None);
    assert_eq!(
        failure(&other, None),
        Failure::Other("the Keychain refused".into())
    );
    let wrong = refused("That request is for something else.", "other");
    assert_eq!(refusal(&wrong, None), None);
    assert!(!wants_state(&other));
    assert!(wants_state(&ended()) && wants_state(&drifted()));
}

#[test]
fn every_refusal_has_its_own_plain_line() {
    let all = [
        Refusal::Expired,
        Refusal::Changed,
        Refusal::VaultOff,
        Refusal::Withdrawn,
    ];
    let lines: Vec<&str> = all.iter().map(|r| r.line()).collect();
    for (i, line) in lines.iter().enumerate() {
        assert!(!line.is_empty());
        assert!(!lines[..i].contains(line), "{line}");
    }
    assert!(lines[2].contains("Vault"));
    assert_eq!(
        Failure::Refused(Refusal::Changed).line(),
        Refusal::Changed.line()
    );
}

#[test]
fn a_save_goes_editing_saving_saved_and_closes_only_when_saved() {
    let mut flow = Flow::default();
    assert!(flow.can_save() && !flow.closes());
    assert!(flow.begin());
    assert_eq!(flow, Flow::Saving);
    assert!(!flow.begin(), "one save at a time");
    flow.settle(Ok(()));
    assert!(flow.closes() && !flow.can_save());
    assert_eq!(flow.line(), None);
}

#[test]
fn a_failed_save_can_be_tried_again_and_shows_the_daemons_words() {
    let mut flow = Flow::default();
    flow.begin();
    flow.settle(Err(Failure::Other("the Keychain refused".into())));
    assert_eq!(flow.line().as_deref(), Some("the Keychain refused"));
    assert!(flow.can_save() && !flow.closes());
    assert!(flow.begin());
}

#[test]
fn a_refusal_of_the_request_ends_saving_but_the_vault_off_can_be_retried() {
    for refusal in [Refusal::Expired, Refusal::Changed, Refusal::Withdrawn] {
        let mut flow = Flow::default();
        flow.begin();
        flow.settle(Err(Failure::Refused(refusal)));
        assert_eq!(flow.line().as_deref(), Some(refusal.line()));
        assert!(!flow.can_save() && !flow.begin(), "{refusal:?}");
        assert!(!flow.closes());
    }
    let mut flow = Flow::default();
    flow.begin();
    flow.settle(Err(Failure::Refused(Refusal::VaultOff)));
    assert!(flow.line().is_some());
    assert!(flow.can_save() && flow.begin());
}
