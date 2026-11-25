use super::login_banner::visible;
use super::outbox_rows::{bubble_line, status_icon, status_line};
use crate::chat::outbox::{bubble, Button, OutboxRow, Status};
use notchling_kit::Appearance;
use serde_json::json;

fn row(state: &str, why: Option<&str>) -> OutboxRow {
    let mut wire = json!({
        "id": "m1", "session": "ab12cd34", "tool": "claude", "kind": "prompt", "text": "go on",
        "state": state, "position": 1, "queuedMs": 10_000, "updatedMs": 10_000, "via": "chat",
    });
    if let Some(why) = why {
        wire["why"] = json!(why);
    }
    OutboxRow::from_wire(&wire).expect("a row")
}

#[test]
fn a_message_held_for_a_signed_out_session_says_sign_in_needed_and_offers_the_terminal() {
    let held = bubble(&row("waiting", Some("login_needed")));
    assert_eq!(held.status, Status::LoginNeeded);
    assert_eq!(
        held.buttons,
        vec![Button::ShowTerminal, Button::Edit, Button::Remove]
    );
    assert_eq!(held.text, "go on");
    assert_eq!(status_line(&Status::LoginNeeded), "Sign in needed");
    assert_eq!(bubble_line(&held, 20_000), "Sign in needed");
}

#[test]
fn the_sign_in_status_wears_the_warning_icon_without_panicking() {
    let _ = status_icon(&Status::LoginNeeded, Appearance::Light);
    let _ = status_icon(&Status::LoginNeeded, Appearance::Dark);
}

#[test]
fn other_waiting_reasons_do_not_become_sign_in_needed() {
    for why in ["retry", "checking", "dialog", "turn", "unreadable"] {
        assert_ne!(
            bubble(&row("waiting", Some(why))).status,
            Status::LoginNeeded,
            "{why}"
        );
    }
}

#[test]
fn the_banner_shows_only_for_a_loaded_session_that_needs_a_login() {
    assert!(visible(true, false, true));
    assert!(!visible(false, false, true));
    assert!(!visible(true, true, true));
    assert!(!visible(true, false, false));
}

#[test]
fn a_why_word_the_app_does_not_know_still_reads_as_a_queued_row() {
    let unknown = row("waiting", Some("a_word_from_a_newer_mac"));
    assert_eq!(unknown.why, None);
    let shown = bubble(&unknown);
    assert_eq!(shown.status, Status::Queued);
    assert_eq!(shown.buttons, vec![Button::Edit, Button::Remove]);
}
