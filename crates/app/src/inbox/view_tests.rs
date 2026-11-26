use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{json, Value};

use super::change::ChangeOpen;
use super::feed::InboxFeed;
use super::model::{ChangeRoute, InboxModel};
use super::triage::{change_busy, key_action, key_target, KeyAction, Section};
use super::view::InboxPanelView;

const NOW: i64 = 10_000_000;

fn raw(id: &str, state: &str, extra: Value) -> Value {
    let mut item = json!({
        "id": id,
        "kind": "consent",
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

fn open_on(id: &str) -> ChangeOpen {
    ChangeOpen {
        id: id.to_owned(),
        route: ChangeRoute::Consent(id.to_owned()),
    }
}

fn view_of(items: Vec<Value>) -> InboxPanelView {
    let feed = Arc::new(InboxFeed::fixed(model_of(items)));
    InboxPanelView::fixed(feed, NOW, Section::Waiting)
}

#[test]
fn an_error_clears_when_its_item_changes_even_if_nothing_polled() {
    let mut view = view_of(vec![raw("a", "open", json!({}))]);
    view.errors
        .record(&view.feed.snapshot(NOW), "a", "The Vault did not answer.");
    view.sync_errors();
    assert!(view.errors.of("a").is_some());
    view.feed = Arc::new(InboxFeed::fixed(model_of(vec![raw(
        "a",
        "answered",
        json!({"answer": "deny"}),
    )])));
    view.sync_errors();
    assert!(view.errors.of("a").is_none());
}

#[test]
fn a_closed_row_or_one_on_another_tab_is_not_a_key_target() {
    let model = model_of(vec![
        raw(
            "blocked",
            "open",
            json!({"blocking": true, "waiting_since": 1}),
        ),
        raw("waits", "open", json!({})),
        raw("done", "answered", json!({"answer": "allow"})),
    ]);
    let triage = model.triage();
    let target = |tab, id| key_target(&triage, tab, id).map(|row| row.id.clone());
    assert_eq!(
        target(Section::Waiting, "blocked"),
        Some("blocked".to_owned())
    );
    assert_eq!(target(Section::Waiting, "waits"), Some("waits".to_owned()));
    assert_eq!(target(Section::Done, "waits"), None);
    assert_eq!(target(Section::Done, "done"), None);
    assert_eq!(target(Section::Waiting, "missing"), None);
}

#[test]
fn keys_do_nothing_while_a_field_has_focus_and_escape_only_cancels_the_note() {
    assert_eq!(key_action("d", true, false, true), KeyAction::Deny);
    assert_eq!(key_action("d", true, false, false), KeyAction::None);
    assert_eq!(key_action("enter", false, false, false), KeyAction::None);
    assert_eq!(key_action("enter", false, false, true), KeyAction::Open);
    assert_eq!(key_action("down", false, false, true), KeyAction::Step(1));
    assert_eq!(key_action("up", false, false, true), KeyAction::Step(-1));
    assert_eq!(
        key_action("escape", false, true, false),
        KeyAction::CancelNote
    );
    assert_eq!(key_action("d", true, true, true), KeyAction::None);
    assert_eq!(key_action("enter", false, true, true), KeyAction::None);
    assert_eq!(key_action("escape", false, false, true), KeyAction::None);
}

#[test]
fn changing_tab_clears_the_focus() {
    let mut view = view_of(vec![raw("waits", "open", json!({}))]);
    view.focus = Some("waits".to_owned());
    view.select_tab(Section::Done);
    assert_eq!(view.focus, None);
    assert_eq!(view.tab, Section::Done);
}

#[test]
fn escape_cancels_the_note_and_tells_chat_it_was_taken_once() {
    let mut view = view_of(vec![raw("a", "open", json!({}))]);
    assert!(!view.cancel_note());
    assert!(!view.took_escape());
    view.change = Some(open_on("a"));
    assert!(view.cancel_note());
    assert!(view.took_escape());
    assert!(!view.took_escape());
}

#[test]
fn send_is_busy_while_allow_or_deny_is_in_flight() {
    let route = ChangeRoute::Consent("c1".to_owned());
    let mut busy = HashSet::new();
    assert!(!change_busy(&busy, "consent:c1:1", &route));
    busy.insert("c1".to_owned());
    assert!(change_busy(&busy, "consent:c1:1", &route));
    let approval = ChangeRoute::Approval("t1".to_owned());
    assert!(!change_busy(&busy, "approval:t1", &approval));
    busy.insert("approval:t1".to_owned());
    assert!(change_busy(&busy, "approval:t1", &approval));
}

#[test]
fn the_note_editor_goes_when_its_row_is_no_longer_open() {
    let mut view = view_of(vec![raw("a", "answered", json!({"answer": "deny"}))]);
    view.change = Some(open_on("a"));
    view.drop_stale_change();
    assert!(!view.change.is_some());
    let mut open = view_of(vec![raw(
        "a",
        "open",
        json!({"alert_actions": ["deny", "change"]}),
    )]);
    open.change = Some(open_on("a"));
    open.drop_stale_change();
    assert!(open.change.is_some());
}

#[test]
fn changing_tab_closes_a_note_whose_row_is_no_longer_shown_but_keeps_a_blocking_one() {
    let mut view = view_of(vec![
        raw("waits", "open", json!({})),
        raw(
            "blocked",
            "open",
            json!({"blocking": true, "waiting_since": 1}),
        ),
    ]);
    view.change = Some(open_on("waits"));
    view.select_tab(Section::Done);
    assert!(view.change.is_none());
    view.change = Some(open_on("blocked"));
    view.select_tab(Section::Waiting);
    assert!(view.change.is_some());
    view.change = Some(open_on("waits"));
    view.select_tab(Section::Waiting);
    assert!(view.change.is_some());
}

#[test]
fn the_key_hint_is_short_enough_for_one_line_at_the_popover_width() {
    let chars = notchling_locale::inbox::keys_hint().chars().count();
    assert!(chars <= 58, "{chars} characters");
}

#[test]
fn a_tab_is_revealed_at_its_place_in_the_row_and_blocking_falls_back_to_the_first() {
    let places: Vec<usize> = Section::TABS.iter().map(|s| s.tab_index()).collect();
    assert_eq!(places, vec![0, 1, 2, 3, 4]);
    assert_eq!(Section::Blocking.tab_index(), 0);
}

#[test]
fn choosing_a_tab_or_focusing_a_row_in_another_one_reveals_that_tab_in_the_row() {
    let mut view = view_of(vec![raw("waits", "open", json!({}))]);
    assert_eq!(view.tab_reveal.map(|r| r.index), Some(view.tab.tab_index()));
    view.select_tab(Section::Info);
    assert_eq!(view.tab_reveal.map(|r| r.index), Some(3));
    view.tab_reveal = None;
    view.select_tab(Section::Done);
    assert_eq!(view.tab_reveal.map(|r| r.index), Some(4));
}
