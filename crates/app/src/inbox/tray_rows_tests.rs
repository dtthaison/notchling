use notchling_engine::inbox::{Action, ActionId, AlertAction, Item, Kind, Place, State};

use super::model::InboxModel;
use super::tray_rows::{attention, consent_id, deny_route, DenyRoute};

const NOW: i64 = 1_000_000_000;

fn item(id: &str) -> Item {
    Item {
        id: id.to_owned(),
        kind: Kind::Consent,
        title: format!("Title {id}"),
        state: State::Open,
        created_ms: NOW - 180_000,
        ..Item::default()
    }
}

fn blocking(id: &str, ends_in_s: Option<i64>) -> Item {
    Item {
        blocking: Some(true),
        expires_ms: ends_in_s.map(|s| NOW + s * 1000),
        ..item(id)
    }
}

fn model(items: Vec<Item>) -> InboxModel {
    InboxModel::from_items(items, NOW)
}

#[test]
fn only_open_blocking_items_are_rows_and_the_rest_wait() {
    let mut closed = blocking("closed", Some(60));
    closed.state = State::Answered;
    let mut calm = item("calm");
    calm.blocking = Some(false);
    let unknown = item("unknown");
    let seen = attention(
        &model(vec![blocking("a", Some(60)), closed, calm, unknown]),
        NOW,
    );
    assert_eq!(seen.blocking.len(), 1);
    assert_eq!(seen.blocking[0].id, "a");
    assert_eq!(seen.waiting_count, 2);
}

#[test]
fn rows_sort_by_the_soonest_end_and_no_end_goes_last() {
    let seen = attention(
        &model(vec![
            blocking("none", None),
            blocking("late", Some(600)),
            blocking("soon", Some(30)),
        ]),
        NOW,
    );
    let ids: Vec<&str> = seen.blocking.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["soon", "late", "none"]);
}

#[test]
fn allow_from_the_menu_follows_the_items_alert_actions() {
    let mut yes = blocking("yes", Some(60));
    yes.alert_actions = vec![AlertAction::Allow, AlertAction::Deny, AlertAction::Change];
    let mut no = blocking("no", Some(90));
    no.alert_actions = vec![AlertAction::Deny, AlertAction::Open, AlertAction::Change];
    let bare = blocking("bare", Some(120));
    let seen = attention(&model(vec![yes, no, bare]), NOW);
    let flags: Vec<bool> = seen.blocking.iter().map(|r| r.allow_from_menu).collect();
    assert_eq!(flags, [true, false, false]);
}

#[test]
fn a_row_says_who_asked_and_how_long_it_has_waited() {
    let mut one = blocking("one", Some(60));
    one.waiting_since = Some(NOW - 120_000);
    one.place = Some(Place {
        session_name: Some("shop".to_owned()),
        ..Place::default()
    });
    let seen = attention(&model(vec![one]), NOW);
    assert_eq!(seen.blocking[0].who, "shop");
    assert_eq!(seen.blocking[0].waited, "2m");
    assert_eq!(seen.blocking[0].title, "Title one");
}

#[test]
fn a_consent_item_names_its_consent() {
    let mut one = item("consent:abc:1");
    one.open = Some(serde_json::json!({"consent": "abc"}));
    assert_eq!(consent_id(&one).as_deref(), Some("abc"));
    assert_eq!(consent_id(&item("plain")), None);
}

fn action(id: ActionId) -> Action {
    Action {
        id,
        label: String::new(),
        primary: false,
    }
}

#[test]
fn a_consent_with_a_deny_denies_through_its_consent() {
    let mut one = blocking("consent:abc:1", None);
    one.open = Some(serde_json::json!({"consent": "abc"}));
    one.alert_actions = vec![AlertAction::Allow, AlertAction::Deny, AlertAction::Change];
    assert_eq!(deny_route(&one), Some(DenyRoute::Consent("abc".to_owned())));
}

#[test]
fn a_secret_approval_denies_through_its_own_answer() {
    let mut one = blocking("vault:1", None);
    one.kind = Kind::SecretApproval;
    one.open = Some(serde_json::json!({"vault_approval": "t1"}));
    one.actions = vec![action(ActionId::AllowOnce), action(ActionId::Deny)];
    one.alert_actions = vec![AlertAction::Deny, AlertAction::Open, AlertAction::Change];
    assert_eq!(deny_route(&one), Some(DenyRoute::Answer(ActionId::Deny)));
}

#[test]
fn an_ask_dismisses_through_its_cancel_answer() {
    let mut one = blocking("ask:q1:1", None);
    one.kind = Kind::Question;
    one.actions = vec![action(ActionId::Open), action(ActionId::Cancel)];
    one.alert_actions = vec![AlertAction::Reply, AlertAction::Dismiss];
    assert_eq!(deny_route(&one), Some(DenyRoute::Answer(ActionId::Cancel)));
}

#[test]
fn an_item_with_no_refusal_has_no_deny_route() {
    let mut one = blocking("note:1", None);
    one.alert_actions = vec![AlertAction::Open];
    assert_eq!(deny_route(&one), None);
    let rows = attention(&model(vec![one]), NOW).blocking;
    assert!(!rows[0].deny_from_menu);
}

#[test]
fn a_row_offers_deny_when_it_has_a_route() {
    let mut one = blocking("consent:abc:1", None);
    one.open = Some(serde_json::json!({"consent": "abc"}));
    one.alert_actions = vec![AlertAction::Deny];
    let rows = attention(&model(vec![one]), NOW).blocking;
    assert!(rows[0].deny_from_menu);
}
