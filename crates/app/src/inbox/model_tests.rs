use super::*;
use notchling_engine::inbox::ActionId;
use serde_json::{json, Value};

fn item(id: &str, created_ms: i64, state: &str, seen: bool) -> Value {
    json!({
        "id": id,
        "kind": "secret_approval",
        "title": format!("Title {id}"),
        "detail": "/work/app",
        "source": {},
        "created_ms": created_ms,
        "expires_ms": null,
        "actions": [
            {"id": "allow_once", "label": "Allow Once", "primary": true},
            {"id": "deny", "label": "Deny", "primary": false}
        ],
        "state": state,
        "answer": null,
        "answered_by": null,
        "seen": seen,
        "open": {"vault_approval": id}
    })
}

fn changed(item: Value) -> Value {
    json!({"event": "inbox.changed", "data": {"item": item}})
}

fn ids(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(|r| r.id.as_str()).collect()
}

#[test]
fn changed_event_upserts_and_orders_newest_first() {
    let mut model = InboxModel::default();
    model.apply(&changed(item("a", 1_000, "open", false)));
    model.apply(&changed(item("b", 3_000, "open", false)));
    model.apply(&changed(item("c", 2_000, "open", false)));
    let (needs, done) = model.sections();
    assert_eq!(ids(&needs), ["b", "c", "a"]);
    assert!(done.is_empty());

    let mut again = item("c", 2_000, "open", true);
    again["title"] = json!("Changed");
    model.apply(&changed(again));
    let (needs, _) = model.sections();
    assert_eq!(ids(&needs), ["b", "c", "a"]);
    assert_eq!(needs[1].title, "Changed");
    assert_eq!(model.unread(), 2);
}

#[test]
fn answered_rows_move_to_done_with_buttons_disabled() {
    let mut model = InboxModel::default();
    model.apply(&changed(item("a", 1_000, "open", false)));
    let (needs, _) = model.sections();
    assert!(needs[0].enabled);
    assert_eq!(
        needs[0].actions,
        vec![
            (ActionId::AllowOnce, "Allow Once".to_owned(), true),
            (ActionId::Deny, "Deny".to_owned(), false),
        ]
    );
    assert_eq!(needs[0].open, Some(json!({"vault_approval": "a"})));

    let mut answered = item("a", 1_000, "answered", true);
    answered["answer"] = json!("allow_once");
    answered["closed_ms"] = json!(2_000);
    model.apply(&changed(answered));
    let (needs, done) = model.sections();
    assert!(needs.is_empty());
    assert_eq!(ids(&done), ["a"]);
    assert!(!done[0].enabled);
    assert_eq!(done[0].actions.len(), 2);
    assert_eq!(done[0].result.as_deref(), Some("Allowed Once"));
    assert_eq!(done[0].detail, "/work/app · Allowed Once");
}

#[test]
fn a_done_row_says_what_was_decided_and_where() {
    let row = |state: &str, answer: Value, by: Value| {
        let mut model = InboxModel::default();
        let mut done = item("a", 1_000, state, true);
        done["answer"] = answer;
        done["answered_by"] = by;
        done["detail"] = json!("");
        model.apply(&changed(done));
        model.sections().1[0].clone()
    };
    for (state, answer, by, said) in [
        (
            "answered",
            json!("allow_once"),
            json!("phone"),
            "Allowed Once · Phone",
        ),
        (
            "answered",
            json!("allow_always"),
            json!("mac"),
            "Always Allowed · Mac",
        ),
        ("answered", json!("allow"), json!("mac"), "Allowed · Mac"),
        ("answered", json!("deny"), json!("phone"), "Denied · Phone"),
        ("answered", json!("cancel"), Value::Null, "Cancelled"),
        ("answered", json!("done"), Value::Null, "Done"),
        ("answered", json!("continue"), Value::Null, "Continued"),
        ("answered", json!("open"), json!("mac"), "Saved · Mac"),
        ("answered", Value::Null, Value::Null, "Answered"),
        ("expired", Value::Null, Value::Null, "Expired"),
        ("withdrawn", Value::Null, Value::Null, "Withdrawn"),
    ] {
        let row = row(state, answer, by);
        assert_eq!(row.result.as_deref(), Some(said), "{state}");
        assert_eq!(row.detail, said, "{state}");
    }
    let mut model = InboxModel::default();
    model.apply(&changed(item("o", 1_000, "open", false)));
    assert_eq!(model.sections().0[0].result, None);
}

#[test]
fn expired_row_says_expired() {
    let mut model = InboxModel::default();
    model.apply(&changed(item("a", 1_000, "expired", true)));
    let (needs, done) = model.sections();
    assert!(needs.is_empty());
    assert_eq!(done[0].detail, "/work/app · Expired");
    assert!(!done[0].enabled);

    let mut bare = item("b", 2_000, "expired", true);
    bare["detail"] = json!("");
    model.apply(&changed(bare));
    let (_, done) = model.sections();
    assert_eq!(done[0].id, "b");
    assert_eq!(done[0].detail, "Expired");
}

#[test]
fn unread_ignores_done() {
    let mut model = InboxModel::default();
    model.apply(&changed(item("a", 1_000, "open", false)));
    model.apply(&changed(item("b", 2_000, "open", true)));
    model.apply(&changed(item("c", 3_000, "answered", false)));
    model.apply(&changed(item("d", 4_000, "expired", false)));
    assert_eq!(model.unread(), 1);
    assert_eq!(model.unseen_ids(), ["a"]);
}

#[test]
fn removed_event_drops_the_row() {
    let mut model = InboxModel::default();
    model.apply(&changed(item("a", 1_000, "open", false)));
    model.apply(&changed(item("b", 2_000, "answered", true)));
    model.apply(&json!({"event": "inbox.removed", "data": {"id": "b"}}));
    let (needs, done) = model.sections();
    assert_eq!(ids(&needs), ["a"]);
    assert!(done.is_empty());
}

#[test]
fn a_list_reply_replaces_what_was_held() {
    let mut model = InboxModel::default();
    model.apply(&changed(item("old", 1_000, "open", false)));
    model.load(&json!({
        "items": [item("x", 5_000, "open", false), item("y", 6_000, "answered", true)],
        "unread": 1
    }));
    let (needs, done) = model.sections();
    assert_eq!(ids(&needs), ["x"]);
    assert_eq!(ids(&done), ["y"]);
}

#[test]
fn done_rows_come_newest_closed_first() {
    let mut model = InboxModel::default();
    let mut first = item("a", 1_000, "answered", true);
    first["closed_ms"] = json!(9_000);
    let mut second = item("b", 2_000, "answered", true);
    second["closed_ms"] = json!(5_000);
    model.apply(&changed(second));
    model.apply(&changed(first));
    let (_, done) = model.sections();
    assert_eq!(ids(&done), ["a", "b"]);
}

#[test]
fn ago_reads_the_clock_the_model_was_given() {
    let mut model = InboxModel::default();
    model.apply(&changed(item("a", 1_000_000, "open", false)));
    model.set_now(1_000_000 + 5 * 60_000);
    assert_eq!(model.sections().0[0].ago, "5m");
    model.set_now(1_000_000 + 30_000);
    assert_eq!(model.sections().0[0].ago, "now");
}

#[test]
fn things_that_are_not_items_change_nothing() {
    let mut model = InboxModel::default();
    model.apply(&json!({"event": "inbox.changed", "data": {"item": {"id": 3}}}));
    model.apply(&json!({"event": "inbox.changed", "data": {}}));
    model.apply(&json!({"event": "inbox.removed", "data": {}}));
    model.apply(&json!({"event": "other", "data": {"item": item("a", 1, "open", false)}}));
    model.apply(&Value::Null);
    model.load(&json!({"nothing": true}));
    assert!(model.is_empty());
}

#[test]
fn a_rows_open_names_the_window_it_brings_up() {
    assert_eq!(
        target(&json!({"vault_approval": "t1"})),
        Some(Target::Approval("t1".to_owned()))
    );
    assert_eq!(
        target(&json!({"vault_request": "t2"})),
        Some(Target::Request("t2".to_owned()))
    );
    assert_eq!(target(&json!({"session": "s"})), None);
    assert_eq!(target(&json!({"vault_request": 5})), None);
    assert_eq!(target(&Value::Null), None);
}

#[test]
fn only_an_open_button_works_without_the_daemon() {
    assert!(is_local(ActionId::Open));
    for action in [
        ActionId::Allow,
        ActionId::AllowOnce,
        ActionId::AllowAlways,
        ActionId::Deny,
        ActionId::Continue,
        ActionId::Cancel,
        ActionId::Done,
    ] {
        assert!(!is_local(action), "{action:?}");
    }
}

#[test]
fn answer_and_seen_calls_name_their_arguments() {
    assert_eq!(
        answer_args("a", ActionId::AllowAlways),
        json!({"id": "a", "action": "allow_always"})
    );
    assert_eq!(
        seen_args(&["a".to_owned(), "b".to_owned()]),
        json!({"ids": ["a", "b"]})
    );
}
