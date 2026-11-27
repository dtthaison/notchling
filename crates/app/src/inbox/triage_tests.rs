use notchling_engine::inbox::{Item, DONE_KEEP_MS};
use serde_json::{json, Value};

use super::model::{ChangeRoute, InboxModel, Tone};
use super::triage::{blocking_cap, blocking_for, panel_max, section_of, RowErrors, Section};

const NOW: i64 = 10_000_000;

fn raw(id: &str, kind: &str, state: &str, extra: Value) -> Value {
    let mut item = json!({
        "id": id,
        "kind": kind,
        "title": format!("Title {id}"),
        "detail": "",
        "source": {},
        "created_ms": NOW - 60_000,
        "expires_ms": null,
        "actions": [
            {"id": "allow", "label": "Allow", "primary": true},
            {"id": "deny", "label": "Deny", "primary": false}
        ],
        "state": state,
        "answer": null,
        "answered_by": null,
        "seen": false,
        "open": {"consent": id}
    });
    for (key, value) in extra.as_object().cloned().unwrap_or_default() {
        item[key] = value;
    }
    item
}

fn model_of(items: Vec<Value>) -> InboxModel {
    let mut model = InboxModel::default();
    model.set_now(NOW);
    for item in items {
        model.apply(&json!({"event": "inbox.changed", "data": {"item": item}}));
    }
    model
}

fn ids(rows: &[super::model::Row]) -> Vec<&str> {
    rows.iter().map(|row| row.id.as_str()).collect()
}

fn item_of(value: Value) -> Item {
    serde_json::from_value(value).expect("an item")
}

#[test]
fn a_blocking_open_item_is_only_in_blocking() {
    let model = model_of(vec![raw(
        "b",
        "consent",
        "open",
        json!({"blocking": true, "waiting_since": NOW - 1000}),
    )]);
    let triage = model.triage();
    assert_eq!(ids(&triage.blocking), ["b"]);
    assert!(triage.waiting.is_empty());
    assert_eq!(triage.blocking[0].section, Section::Blocking);
}

#[test]
fn an_open_item_that_is_not_blocking_and_wants_an_answer_is_only_waiting() {
    let model = model_of(vec![raw("w", "consent", "open", json!({}))]);
    let triage = model.triage();
    assert_eq!(ids(&triage.waiting), ["w"]);
    assert!(triage.blocking.is_empty() && triage.info.is_empty());
}

#[test]
fn a_failed_outcome_is_in_didnt_work_and_says_why() {
    let model = model_of(vec![raw(
        "f",
        "consent",
        "answered",
        json!({"answer": "allow", "closed_ms": NOW - 5000, "seen": true,
               "outcome": {"state": "failed", "reason": "the machine is gone", "at_ms": NOW - 5000}}),
    )]);
    let triage = model.triage();
    assert_eq!(ids(&triage.didnt_work), ["f"]);
    assert!(triage.done.is_empty());
    assert_eq!(
        triage.didnt_work[0].pill.text,
        "Failed: the machine is gone"
    );
    assert_eq!(triage.didnt_work[0].pill.tone, Tone::Bad);
}

#[test]
fn an_applying_outcome_is_running_and_a_withdrawn_one_did_not_work() {
    let model = model_of(vec![
        raw(
            "r",
            "consent",
            "answered",
            json!({"answer": "allow", "closed_ms": NOW, "seen": true,
                   "outcome": {"state": "applying", "at_ms": NOW}}),
        ),
        raw(
            "x",
            "consent",
            "answered",
            json!({"answer": "allow", "closed_ms": NOW, "seen": true,
                   "outcome": {"state": "withdrawn", "at_ms": NOW}}),
        ),
    ]);
    let triage = model.triage();
    assert_eq!(ids(&triage.running), ["r"]);
    assert_eq!(triage.running[0].pill.text, "Applying");
    assert_eq!(ids(&triage.didnt_work), ["x"]);
}

#[test]
fn an_expired_consent_secret_or_session_wait_did_not_work() {
    for kind in [
        "consent",
        "secret_approval",
        "secret_request",
        "session_waits",
    ] {
        let one = item_of(raw(
            "e",
            kind,
            "expired",
            json!({"closed_ms": NOW - 10, "seen": true}),
        ));
        assert_eq!(section_of(&one, NOW), Section::DidntWork, "{kind}");
    }
}

#[test]
fn an_expired_limit_notice_is_done() {
    let one = item_of(raw(
        "l",
        "limit_notice",
        "expired",
        json!({"closed_ms": NOW - 10, "seen": true, "actions": [], "open": null}),
    ));
    assert_eq!(section_of(&one, NOW), Section::Done);
}

#[test]
fn a_noted_deny_is_done_and_reads_sent_back() {
    let model = model_of(vec![raw(
        "n",
        "consent",
        "answered",
        json!({"answer": "deny", "closed_ms": NOW - 10, "seen": true, "note": "use k2"}),
    )]);
    let triage = model.triage();
    assert_eq!(ids(&triage.done), ["n"]);
    assert_eq!(triage.done[0].pill.text, "Sent back");
    assert_eq!(triage.done[0].detail, "Sent back: use k2");
}

#[test]
fn an_open_item_with_nothing_to_answer_is_info_until_it_is_seen() {
    let done_only =
        json!({"actions": [{"id": "done", "label": "Done", "primary": true}], "open": null});
    let mut unseen = done_only.clone();
    unseen["seen"] = json!(false);
    let mut seen = done_only;
    seen["seen"] = json!(true);
    assert_eq!(
        section_of(&item_of(raw("i", "limit_notice", "open", unseen)), NOW),
        Section::Info
    );
    assert_eq!(
        section_of(&item_of(raw("i", "limit_notice", "open", seen)), NOW),
        Section::Done
    );
}

#[test]
fn a_didnt_work_row_leaves_after_the_keep_time() {
    let old = item_of(raw(
        "o",
        "consent",
        "answered",
        json!({"answer": "allow", "closed_ms": NOW - DONE_KEEP_MS - 1, "seen": true,
               "outcome": {"state": "failed", "at_ms": NOW - DONE_KEEP_MS - 1}}),
    ));
    assert_eq!(section_of(&old, NOW), Section::Done);
}

#[test]
fn the_badge_counts_blocking_and_waiting_and_not_unseen_info() {
    let info = |id: &str| {
        raw(
            id,
            "limit_notice",
            "open",
            json!({"actions": [{"id": "done", "label": "Done", "primary": true}], "open": null}),
        )
    };
    let blocking = |id: &str| {
        raw(
            id,
            "consent",
            "open",
            json!({"blocking": true, "waiting_since": 1}),
        )
    };
    let model = model_of(vec![
        blocking("b1"),
        blocking("b2"),
        raw("w", "consent", "open", json!({})),
        info("i1"),
        info("i2"),
        info("i3"),
    ]);
    assert_eq!(model.unread(), 3);
    assert_eq!(model.triage().info.len(), 3);
}

#[test]
fn clear_moves_a_didnt_work_row_to_done() {
    let mut model = model_of(vec![raw(
        "f",
        "consent",
        "answered",
        json!({"answer": "allow", "closed_ms": NOW - 5000, "seen": true,
               "outcome": {"state": "failed", "at_ms": NOW - 5000}}),
    )]);
    assert_eq!(model.triage().didnt_work.len(), 1);
    model.clear_row("f");
    let triage = model.triage();
    assert!(triage.didnt_work.is_empty());
    assert_eq!(ids(&triage.done), ["f"]);
}

#[test]
fn a_row_error_stays_while_another_row_changes_and_clears_when_its_own_row_changes() {
    let mut model = model_of(vec![
        raw("a", "consent", "open", json!({})),
        raw("b", "consent", "open", json!({})),
    ]);
    let mut errors = RowErrors::default();
    errors.record(&model, "a", "the daemon said no");
    let mut other = raw("b", "consent", "open", json!({}));
    other["title"] = json!("Another title");
    model.apply(&json!({"event": "inbox.changed", "data": {"item": other}}));
    errors.sync(&model);
    assert_eq!(errors.of("a"), Some("the daemon said no"));
    assert_eq!(errors.of("b"), None);
    let mut mine = raw("a", "consent", "open", json!({}));
    mine["seen"] = json!(true);
    model.apply(&json!({"event": "inbox.changed", "data": {"item": mine}}));
    errors.sync(&model);
    assert_eq!(errors.of("a"), None);
}

#[test]
fn a_row_error_goes_when_its_row_does() {
    let mut model = model_of(vec![raw("a", "consent", "open", json!({}))]);
    let mut errors = RowErrors::default();
    errors.record(&model, "a", "no");
    model.apply(&json!({"event": "inbox.removed", "data": {"id": "a"}}));
    errors.sync(&model);
    assert_eq!(errors.of("a"), None);
}

#[test]
fn the_chat_strip_shows_for_the_blocked_session_only() {
    let model = model_of(vec![
        raw(
            "b",
            "consent",
            "open",
            json!({"blocking": true, "waiting_since": 1, "place": {"session": "s1"}}),
        ),
        raw(
            "c",
            "consent",
            "open",
            json!({"blocking": true, "waiting_since": 1, "source": {"session": "s3"}}),
        ),
        raw("w", "consent", "open", json!({"place": {"session": "s2"}})),
    ]);
    assert_eq!(
        blocking_for(&model, None, &["s1"]).map(|s| s.id),
        Some("b".to_owned())
    );
    assert_eq!(
        blocking_for(&model, None, &["s3"]).map(|s| s.id),
        Some("c".to_owned())
    );
    assert!(blocking_for(&model, None, &["s2"]).is_none());
    assert!(blocking_for(&model, None, &["s9"]).is_none());
}

#[test]
fn a_row_names_machine_session_and_branch_and_how_long_it_has_waited() {
    let model = model_of(vec![raw(
        "b",
        "consent",
        "open",
        json!({"blocking": true, "waiting_since": NOW - 180_000, "expires_ms": NOW + 720_000,
               "place": {"machine_name": "m1", "session_name": "notchling", "branch": "pr-prep"}}),
    )]);
    let row = &model.triage().blocking[0];
    assert_eq!(row.machine.as_deref(), Some("m1"));
    assert_eq!(row.session.as_deref(), Some("notchling · pr-prep"));
    assert_eq!(row.waited.as_deref(), Some("stopped 3m"));
    assert!(row.pill.drain.is_some_and(|left| left > 0.0 && left < 1.0));
}

#[test]
fn change_goes_where_the_deny_goes_for_each_kind_that_allows_it() {
    let alerts = json!(["deny", "open", "change"]);
    let consent = model_of(vec![raw(
        "c",
        "consent",
        "open",
        json!({"alert_actions": alerts}),
    )]);
    assert_eq!(
        consent.triage().waiting[0].change,
        Some(ChangeRoute::Consent("c".to_owned()))
    );
    let approval = model_of(vec![raw(
        "p",
        "secret_approval",
        "open",
        json!({"alert_actions": alerts, "open": {"vault_approval": "t1"}}),
    )]);
    assert_eq!(
        approval.triage().waiting[0].change,
        Some(ChangeRoute::Approval("t1".to_owned()))
    );
    let unverified = model_of(vec![raw(
        "u",
        "consent",
        "open",
        json!({"alert_actions": ["deny", "open"]}),
    )]);
    assert_eq!(unverified.triage().waiting[0].change, None);
}

#[test]
fn an_open_item_whose_outcome_is_applying_is_running_and_not_blocking() {
    let model = model_of(vec![raw(
        "a",
        "consent",
        "open",
        json!({
            "blocking": true,
            "waiting_since": NOW - 1000,
            "place": {"session": "s1"},
            "outcome": {"state": "applying", "at_ms": NOW}
        }),
    )]);
    let triage = model.triage();
    assert_eq!(ids(&triage.running), ["a"]);
    assert!(triage.blocking.is_empty() && triage.waiting.is_empty());
    assert!(!triage.running[0].enabled);
    assert_eq!(model.unread(), 0);
    assert!(blocking_for(&model, None, &["s1"]).is_none());
}

#[test]
fn every_expired_consent_did_not_work_whatever_its_kind() {
    for kind in ["setting_change", "consent", "notice", "login_needed"] {
        let model = model_of(vec![raw(
            "x",
            kind,
            "expired",
            json!({"closed_ms": NOW - 1000}),
        )]);
        assert_eq!(
            section_of(model.item("x").expect("item"), NOW),
            Section::DidntWork,
            "{kind}"
        );
    }
    let notice = model_of(vec![raw(
        "n",
        "limit_notice",
        "expired",
        json!({"open": null}),
    )]);
    assert_eq!(
        section_of(notice.item("n").expect("item"), NOW),
        Section::Done
    );
}

#[test]
fn a_mirrored_item_with_no_open_has_no_change() {
    let model = model_of(vec![raw(
        "m",
        "consent",
        "open",
        json!({"open": null, "alert_actions": ["deny", "change"]}),
    )]);
    assert_eq!(model.triage().waiting[0].change, None);
}

#[test]
fn the_chat_strip_matches_only_the_machine_of_the_selected_session() {
    let node = "00000000000000a1";
    let other = "00000000000000b2";
    let model = model_of(vec![
        raw(
            "remote",
            "consent",
            "open",
            json!({"blocking": true, "waiting_since": 1, "source": {"node": node, "session": "s1"}}),
        ),
        raw(
            "placed",
            "consent",
            "open",
            json!({"blocking": true, "waiting_since": 1, "place": {"machine": other, "session": "s1"}}),
        ),
    ]);
    assert!(blocking_for(&model, None, &["s1"]).is_none());
    assert_eq!(
        blocking_for(&model, Some(node), &["s1"]).map(|s| s.id),
        Some("remote".to_owned())
    );
    assert_eq!(
        blocking_for(&model, Some(other), &["s1"]).map(|s| s.id),
        Some("placed".to_owned())
    );
    assert!(blocking_for(&model, Some("00000000000000c3"), &["s1"]).is_none());
}

#[test]
fn the_panel_never_gets_a_zero_or_negative_height() {
    for viewport in [-10.0, 0.0, 30.0, 56.0, 100.0, 200.0] {
        let room = panel_max(viewport);
        assert!(room >= super::PANEL_MIN, "{viewport}: {room}");
    }
    assert_eq!(panel_max(576.0), 520.0);
    assert_eq!(panel_max(5000.0), super::MAX_HEIGHT);
}

#[test]
fn the_pinned_section_always_leaves_the_tabs_a_real_share() {
    let mut room = super::PANEL_MIN;
    while room <= super::MAX_HEIGHT {
        let cap = blocking_cap(room);
        assert!(cap >= 0.0, "{room}: {cap}");
        assert!(
            room - cap >= super::PANEL_CHROME + super::TABS_SHARE,
            "{room}: cap {cap}"
        );
        room += 1.0;
    }
    assert_eq!(blocking_cap(404.0), 184.0);
    assert_eq!(blocking_cap(520.0), 300.0);
    assert_eq!(blocking_cap(524.0), 300.0);
}

#[test]
fn the_pinned_section_fits_two_tall_cards_in_the_biggest_panel() {
    assert_eq!(blocking_cap(super::MAX_HEIGHT), 630.0);
    assert!(blocking_cap(700.0) < blocking_cap(800.0));
}
