use notchling_engine::inbox::{AlertAction, Item, Kind, State};

use super::super::blocking::Closing;
use super::super::layout::NotchState;
use super::super::model::{NotchModel, HOVER_GRACE_MS};
use crate::attention::{BlockingTarget, Effect};

const NOW: i64 = 1_000_000_000;

fn item(id: &str, ends_in_min: Option<i64>) -> Item {
    Item {
        id: id.to_owned(),
        kind: Kind::Consent,
        title: format!("Title {id}"),
        state: State::Open,
        created_ms: NOW - 180_000,
        blocking: Some(true),
        waiting_since: Some(NOW - 120_000),
        expires_ms: ends_in_min.map(|m| NOW + m * 60_000),
        alert_actions: vec![AlertAction::Allow, AlertAction::Deny, AlertAction::Change],
        ..Item::default()
    }
}

fn model_with(items: Vec<Item>) -> NotchModel {
    let mut model = NotchModel::default();
    model.now_ms = NOW;
    model.set_inbox_items(&items, NOW);
    model
}

fn open(id: &str, ms: i64, stay: bool) -> Effect {
    Effect::OpenNotch {
        key: BlockingTarget::Item(id.to_owned()),
        ms,
        stay,
    }
}

#[test]
fn hovering_in_and_out_of_a_timed_card_still_closes_it_at_its_time() {
    let mut m = model_with(vec![item("a", None)]);
    m.open_blocking("a", 8_000, false, NOW);
    m.hover(true, NOW + 1_000);
    m.tick(NOW + 5_000);
    assert_eq!(m.state, NotchState::Blocking);
    m.hover(false, NOW + 2_000 + 3_000);
    m.tick(NOW + 7_999);
    assert_eq!(m.state, NotchState::Blocking);
    m.tick(NOW + 8_000);
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn leaving_after_the_time_has_passed_closes_after_a_short_grace() {
    let mut m = model_with(vec![item("a", None)]);
    m.open_blocking("a", 8_000, false, NOW);
    m.hover(true, NOW + 1_000);
    m.tick(NOW + 20_000);
    assert_eq!(m.state, NotchState::Blocking);
    m.hover(false, NOW + 20_000);
    m.tick(NOW + 20_000 + HOVER_GRACE_MS - 1);
    assert_eq!(m.state, NotchState::Blocking);
    assert!(m.tick(NOW + 20_000 + HOVER_GRACE_MS));
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn hovering_a_glance_and_leaving_closes_it_too() {
    let mut m = model_with(vec![item("a", None)]);
    m.grow_blocking("a", NOW);
    assert_eq!(m.state, NotchState::Glance);
    m.hover(true, NOW + 500);
    m.hover(false, NOW + 6_000);
    m.tick(NOW + 6_000 + HOVER_GRACE_MS);
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn an_urgent_card_stays_after_hovering() {
    let mut m = model_with(vec![item("a", None)]);
    m.open_blocking("a", 45_000, true, NOW);
    m.hover(true, NOW + 1_000);
    m.hover(false, NOW + 2_000);
    m.tick(NOW + 600_000);
    assert_eq!(m.state, NotchState::Blocking);
}

#[test]
fn switching_cards_with_an_open_effect_restarts_the_allow_guard() {
    let mut m = model_with(vec![item("a", Some(2)), item("b", Some(9))]);
    m.apply_effect(&open("a", 45_000, true), NOW);
    assert!(m.blocking.allow_ready(NOW + 5_000));
    m.apply_effect(&open("b", 45_000, true), NOW + 5_000);
    assert!(!m.blocking.allow_ready(NOW + 5_500));
    assert!(m.blocking.allow_ready(NOW + 6_000));
}

#[test]
fn the_next_card_after_a_farewell_restarts_the_allow_guard() {
    let mut m = model_with(vec![item("a", Some(2)), item("b", Some(9))]);
    m.apply_effect(&open("a", 45_000, true), NOW);
    m.set_inbox_items(&[item("b", Some(9))], NOW + 1_000);
    assert!(m.blocking.farewell_live(NOW + 1_500).is_some());
    m.tick(NOW + 3_000);
    assert!(m.blocking.farewell_live(NOW + 3_000).is_none());
    assert!(!m.blocking.allow_ready(NOW + 3_500));
    assert!(m.blocking.allow_ready(NOW + 4_000));
}

#[test]
fn an_expired_card_opens_a_closed_notch_for_four_seconds_though_it_left_the_cards() {
    let mut m = model_with(vec![item("a", None)]);
    let mut gone = item("a", None);
    gone.state = State::Expired;
    m.set_inbox_items(&[gone], NOW + 1_000);
    assert!(m.blocking.is_empty());
    m.apply_effect(&open("a", 4_000, false), NOW + 1_050);
    assert_eq!(m.state, NotchState::Blocking);
    let farewell = m.blocking.farewell_live(NOW + 5_000).unwrap();
    assert_eq!(farewell.kind, Closing::Expired);
    m.tick(NOW + 1_050 + 3_999);
    assert_eq!(m.state, NotchState::Blocking);
    assert!(m.tick(NOW + 1_050 + 4_000));
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn an_expired_card_that_is_not_the_current_one_still_says_expired() {
    let mut m = model_with(vec![item("a", Some(2)), item("b", Some(9))]);
    let mut gone = item("b", Some(9));
    gone.state = State::Expired;
    m.set_inbox_items(&[item("a", Some(2)), gone], NOW + 1_000);
    m.apply_effect(&open("b", 4_000, false), NOW + 1_050);
    assert_eq!(m.state, NotchState::Blocking);
    assert_eq!(
        m.blocking.farewell_live(NOW + 2_000).map(|f| f.kind),
        Some(Closing::Expired)
    );
}

#[test]
fn a_new_card_cancels_a_close_waiting_on_an_old_farewell() {
    let mut m = model_with(vec![item("a", Some(1)), item("b", Some(30))]);
    m.apply_effect(&open("a", 45_000, true), NOW);
    m.set_inbox_items(&[item("b", Some(30))], NOW + 1_000);
    m.apply_effect(&Effect::CloseNotch, NOW + 1_000);
    assert!(m.blocking.close_pending);
    m.apply_effect(&open("b", 45_000, true), NOW + 1_500);
    assert!(!m.blocking.close_pending);
    m.tick(NOW + 3_000);
    m.tick(NOW + 4_000);
    assert_eq!(m.state, NotchState::Blocking);
}
