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
    assert_eq!(model.unread(), 3);
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
fn inbox_answered_words_for_send_and_discard() {
    for (answer, by, said) in [
        ("send", json!("mac"), "Sent · Mac"),
        ("discard", json!("phone"), "Discarded · Phone"),
        ("send", Value::Null, "Sent"),
        ("discard", Value::Null, "Discarded"),
    ] {
        let mut model = InboxModel::default();
        let mut done = item("a", 1_000, "answered", true);
        done["answer"] = json!(answer);
        done["answered_by"] = by;
        done["detail"] = json!("");
        model.apply(&changed(done));
        assert_eq!(model.sections().1[0].result.as_deref(), Some(said));
    }
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
fn unread_counts_what_waits_and_ignores_done_and_unseen() {
    let mut model = InboxModel::default();
    model.apply(&changed(item("a", 1_000, "open", false)));
    model.apply(&changed(item("b", 2_000, "open", true)));
    model.apply(&changed(item("c", 3_000, "answered", false)));
    model.apply(&changed(item("d", 4_000, "expired", false)));
    assert_eq!(model.unread(), 2);
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

fn question(state: &str, answer: Option<&str>, by: Option<&str>) -> Value {
    json!({
        "id": "ask:q1:5",
        "kind": "question",
        "title": "Review asks",
        "detail": "Which one?",
        "source": {"session": "ab12cd34"},
        "created_ms": 1_000,
        "expires_ms": null,
        "actions": [
            {"id": "choice_1", "label": "Keep it", "primary": false},
            {"id": "choice_2", "label": "Drop it", "primary": false},
            {"id": "open", "label": "Answer in Chat", "primary": false},
            {"id": "cancel", "label": "Dismiss", "primary": false}
        ],
        "state": state,
        "answer": answer,
        "answered_by": by,
        "seen": true,
        "open": {"ask": "q1", "session": "ab12cd34"}
    })
}

fn result_of_question(state: &str, answer: Option<&str>, by: Option<&str>) -> Option<String> {
    let mut model = InboxModel::default();
    model.apply(&changed(question(state, answer, by)));
    let (needs, done) = model.sections();
    needs.into_iter().chain(done).next().and_then(|r| r.result)
}

#[test]
fn inbox_question_answered_words() {
    let word = |answer, by| result_of_question("answered", answer, by);
    assert_eq!(
        word(Some("choice_2"), Some("mac")).as_deref(),
        Some("Answered · Mac: Drop it")
    );
    assert_eq!(
        word(Some("choice_1"), Some("phone")).as_deref(),
        Some("Answered · Phone: Keep it")
    );
    assert_eq!(word(None, Some("mac")).as_deref(), Some("Answered in Chat"));
    assert_eq!(word(None, None).as_deref(), Some("Answered in Chat"));
    assert_eq!(
        word(Some("cancel"), Some("mac")).as_deref(),
        Some("Dismissed")
    );
    assert_eq!(result_of_question("open", None, None), None);
    assert_eq!(
        result_of_question("withdrawn", None, None).as_deref(),
        Some("Withdrawn")
    );
}

#[test]
fn inbox_open_on_question_goes_to_chat() {
    let mut model = InboxModel::default();
    model.apply(&changed(question("open", None, None)));
    let (needs, _) = model.sections();
    let open = needs[0].open.as_ref().expect("an open target");
    assert_eq!(
        target(open),
        Some(Target::Ask {
            id: "q1".to_owned(),
            session: "ab12cd34".to_owned()
        })
    );
    assert_eq!(target(&json!({"ask": "q1"})), None, "no session, no card");
    assert!(is_local(ActionId::Open));
}

#[test]
fn consent_requests_are_held_by_id_and_dropped_on_loss() {
    let mut model = InboxModel::default();
    model.load_consents(&json!({"requests": [{"id": "c1"}, {"id": "c2"}]}));
    assert!(model.consent("c1").is_some());
    assert!(model.consent("c3").is_none());
    model.load_consents(&json!({"nothing": true}));
    assert!(
        model.consent("c2").is_some(),
        "a reply without requests changes nothing"
    );
    model.clear();
    assert!(model.consent("c1").is_none());
}

#[test]
fn a_consent_row_names_its_request_and_the_calls_name_their_arguments() {
    assert_eq!(
        consent_of(&json!({"consent": "c1", "vault_request": "c1"})),
        Some("c1")
    );
    assert_eq!(consent_of(&json!({"ask": "q"})), None);
    assert_eq!(
        consent_answer_args("c1", true),
        json!({"consentId": "c1", "decision": "allow"})
    );
    assert_eq!(
        consent_answer_args("c1", false),
        json!({"consentId": "c1", "decision": "deny"})
    );
    assert_eq!(
        consent_deny_with_args("c1", "rename it"),
        json!({"consentId": "c1", "decision": "deny", "note": "rename it"})
    );
}

#[test]
fn a_row_carries_the_node_its_item_came_from() {
    let mut raised = item("consent:c1:5", 1_000, "open", false);
    raised["source"] = json!({"node": "m1"});
    let mut model = InboxModel::default();
    model.load(&json!({"items": [raised]}));
    let (needs, _) = model.sections();
    assert_eq!(needs[0].node.as_deref(), Some("m1"));
}

fn mirrored_switch(kind: &str) -> Value {
    let node = "ab12cd34".repeat(2);
    let mut raised = item(&format!("{node}/consent:c9:5"), 1_000, "open", false);
    raised["kind"] = json!(kind);
    raised["title"] = json!("Claude Code asks to tidy the docs");
    raised["detail"] = json!("Nothing to worry about.");
    raised["open"] = Value::Null;
    raised["switch"] = json!("bridge");
    raised["source"] = json!({"node": "trusted-looking"});
    raised
}

#[test]
fn a_workers_switch_request_shows_the_macs_own_words_not_the_workers() {
    let mut model = InboxModel::default();
    model.load(&json!({"items": [mirrored_switch("consent")]}));
    let (needs, _) = model.sections();
    assert_eq!(
        needs[0].title,
        t::inbox::worker_switch_title(t::consent::card_switch_bridge(), "ab12cd34")
    );
    assert_eq!(needs[0].detail, t::consent::card_switch_bridge_unlocks());
    assert!(!needs[0].title.contains("docs"));
    assert!(!needs[0].detail.contains("worry"));
}

#[test]
fn an_item_that_is_not_a_switch_request_keeps_its_words() {
    let mut model = InboxModel::default();
    model.load(&json!({"items": [mirrored_switch("notice")]}));
    let (needs, done) = model.sections();
    let row = needs.iter().chain(done.iter()).next().unwrap();
    assert_eq!(row.title, "Claude Code asks to tidy the docs");
}

#[test]
fn a_cluster_apply_carries_the_consent_id_beside_its_args() {
    let args = json!({"node": "n-spare", "name": "spare-box"});
    assert_eq!(
        cluster_apply_args("c9", &args),
        json!({"node": "n-spare", "name": "spare-box", "consentId": "c9"})
    );
    assert_eq!(args, json!({"node": "n-spare", "name": "spare-box"}));
}

#[test]
fn account_kind_is_app_applied() {
    let mut model = InboxModel::default();
    model.load_consents(&json!({"requests": [{
        "id": "c10", "kind": "account", "state": "waiting", "appliedBy": "app", "answerOn": "mac",
        "expiresMs": 9_999_999_999_999i64,
        "change": {"action": "switch", "tool": "claude",
                   "apply": {"op": "accounts.switch", "args": {"id": "acct-2222222222222222"}}}
    }]}));
    let view = model.consent("c10").expect("request");
    let card = crate::inbox::consent_card::ConsentCard::build(
        view,
        &crate::inbox::consent_card::Env::default(),
    )
    .expect("card");
    assert!(card.app_applied);
    let apply = card.account_apply.expect("apply");
    assert_eq!(apply.op, "accounts.switch");
    assert_eq!(
        cluster_apply_args("c10", &apply.args),
        json!({"id": "acct-2222222222222222", "consentId": "c10"})
    );
}
