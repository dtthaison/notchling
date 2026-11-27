use notchling_engine::inbox::{AlertAction, Item, Kind, Place, State};

use super::*;
use crate::notch::layout::{blocking_cap, NotchState};
use crate::notch::model::NotchModel;
use crate::notch::prefs::NotchPrefs;
use crate::notch::requests::{HookRequest, RequestKind};

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

fn hook(id: &str) -> HookRequest {
    HookRequest {
        id: id.to_owned(),
        session_id: "s9".to_owned(),
        created: (NOW - 30_000) as f64 / 1000.0,
        kind: RequestKind::Permission,
        tool_name: "Bash".to_owned(),
        summary: "ls".to_owned(),
        description: String::new(),
        can_always_allow: false,
        agy: None,
    }
}

fn model_with(items: Vec<Item>) -> NotchModel {
    let mut model = NotchModel::default();
    model.now_ms = NOW;
    model.set_inbox_items(&items, NOW);
    model
}

fn ids(model: &NotchModel) -> Vec<String> {
    model
        .blocking
        .cards()
        .iter()
        .map(|c| c.id.clone())
        .collect()
}

#[test]
fn three_blocking_items_come_soonest_end_first_and_the_stepper_wraps() {
    let mut m = model_with(vec![
        item("five", Some(5)),
        item("two", Some(2)),
        item("nine", Some(9)),
    ]);
    assert_eq!(ids(&m), ["two", "five", "nine"]);
    assert_eq!(m.blocking.current().unwrap().id, "two");
    m.step_blocking(true, NOW);
    m.step_blocking(true, NOW);
    assert_eq!(m.blocking.current().unwrap().id, "nine");
    m.step_blocking(true, NOW);
    assert_eq!(m.blocking.current().unwrap().id, "two");
    m.step_blocking(false, NOW);
    assert_eq!(m.blocking.current().unwrap().id, "nine");
    assert_eq!(m.blocking.position(), 2);
}

#[test]
fn only_open_blocking_items_count_and_waiting_ones_stay_out() {
    let mut calm = item("calm", None);
    calm.blocking = Some(false);
    let mut closed = item("closed", None);
    closed.state = State::Answered;
    let mut asking = item("asking", None);
    asking.waiter = Some(notchling_engine::inbox::Waiter::Ask);
    let m = model_with(vec![item("a", Some(3)), calm, closed, asking]);
    assert_eq!(ids(&m), ["a"]);
    assert_eq!(m.blocking.count(), 1);
}

#[test]
fn a_setting_has_allow_deny_change_and_later() {
    let card = card_of_item(&item("s", None), true, String::new(), String::new());
    assert_eq!(
        card.buttons,
        [Button::Allow, Button::Deny, Button::Change, Button::Later]
    );
}

#[test]
fn a_model_item_has_no_allow() {
    let mut model = item("m", None);
    model.alert_actions = vec![AlertAction::Deny, AlertAction::Open, AlertAction::Change];
    for on in [true, false] {
        let card = card_of_item(&model, on, String::new(), String::new());
        assert!(!card.buttons.contains(&Button::Allow));
        assert_eq!(
            card.buttons,
            [Button::Deny, Button::Open, Button::Change, Button::Later]
        );
    }
}

#[test]
fn answer_from_the_notch_off_replaces_allow_with_open() {
    let card = card_of_item(&item("s", None), false, String::new(), String::new());
    assert_eq!(
        card.buttons,
        [Button::Open, Button::Deny, Button::Change, Button::Later]
    );
    let mut m = model_with(vec![item("s", None)]);
    m.apply(&NotchPrefs {
        answer: false,
        ..NotchPrefs::default()
    });
    assert!(!m.blocking.cards()[0].buttons.contains(&Button::Allow));
}

#[test]
fn a_secret_request_opens_its_form_and_an_unverified_caller_denies_or_opens() {
    let mut request = item("r", None);
    request.alert_actions = vec![
        AlertAction::OpenForm,
        AlertAction::Cancel,
        AlertAction::Change,
    ];
    assert_eq!(
        item_buttons(&request, true),
        [Button::OpenForm, Button::Later]
    );
    let mut stranger = item("u", None);
    stranger.alert_actions = vec![AlertAction::Deny, AlertAction::Open];
    assert_eq!(
        item_buttons(&stranger, true),
        [Button::Deny, Button::Open, Button::Later]
    );
    let mut bare = item("b", None);
    bare.alert_actions = Vec::new();
    assert_eq!(item_buttons(&bare, true), [Button::Open, Button::Later]);
}

#[test]
fn a_hook_prompt_answered_on_screen_has_no_change() {
    assert_eq!(
        hook_buttons(true, true),
        [Button::Allow, Button::Deny, Button::Change, Button::Later]
    );
    assert_eq!(
        hook_buttons(false, true),
        [Button::Allow, Button::Deny, Button::Later]
    );
}

#[test]
fn a_held_hook_prompt_joins_the_cards_and_an_answered_one_leaves() {
    let mut m = model_with(vec![item("a", Some(3))]);
    m.requests.insert("s9".into(), hook("r1"));
    m.rebuild_blocking(NOW);
    assert_eq!(ids(&m), ["a", "r1"]);
    m.mark_answered_key("r1");
    m.rebuild_blocking(NOW);
    assert_eq!(ids(&m), ["a"]);
}

#[test]
fn a_draft_note_survives_the_notch_closing_and_the_card_reopening() {
    let mut m = model_with(vec![item("a", Some(3)), item("b", Some(4))]);
    m.open_blocking("a", 8_000, false, NOW);
    m.blocking.set_draft("a", "use the staging key");
    m.close_blocking();
    assert_eq!(m.state, NotchState::Collapsed);
    m.open_blocking("a", 8_000, true, NOW + 1_000);
    assert_eq!(m.blocking.draft("a"), "use the staging key");
    assert_eq!(m.blocking.draft("b"), "");
    m.set_inbox_items(&[item("a", Some(3))], NOW + 2_000);
    assert_eq!(m.blocking.draft("a"), "use the staging key");
}

#[test]
fn a_draft_is_cut_at_the_note_limit_and_dropped_with_its_item() {
    let mut m = model_with(vec![item("a", None)]);
    m.blocking.set_draft("a", &"x".repeat(400));
    assert_eq!(m.blocking.draft("a").chars().count(), NOTE_CHARS);
    m.set_inbox_items(&[], NOW);
    assert_eq!(m.blocking.draft("a"), "");
}

#[test]
fn the_words_say_stopped_in_minutes_and_the_seconds_left_in_the_last_minute() {
    let m = model_with(vec![item("a", Some(10))]);
    let card = &m.blocking.cards()[0];
    assert_eq!(time_words(card, NOW), "stopped 2m");
    let mut last = card.clone();
    last.ends_ms = Some(NOW + 52_000);
    assert_eq!(time_words(&last, NOW), "ends in 0:52");
    assert!(last_call(&last, NOW));
    assert!(!last_call(card, NOW));
    last.ends_ms = Some(NOW + 61_000);
    assert_eq!(time_words(&last, NOW), "stopped 2m");
    let mut fresh = card.clone();
    fresh.since_ms = NOW - 4_000;
    assert_eq!(time_words(&fresh, NOW), "stopped 0:04");
}

#[test]
fn the_card_says_who_by_session_name_branch_and_machine() {
    let mut one = item("a", None);
    one.place = Some(Place {
        session_name: Some("pr-prep-merge".into()),
        branch: Some("agent/x".into()),
        machine_name: Some("m1".into()),
        ..Place::default()
    });
    let card = card_of_item(&one, true, String::new(), String::new());
    assert_eq!(card.who, ["pr-prep-merge", "agent/x", "m1"]);
}

#[test]
fn allow_waits_a_second_after_the_card_shows() {
    let mut m = model_with(vec![item("a", None), item("b", None)]);
    m.open_blocking("a", 8_000, true, NOW);
    assert!(!m.blocking.allow_ready(NOW + 999));
    assert!(m.blocking.allow_ready(NOW + 1_000));
    m.open_blocking("a", 8_000, true, NOW + 5_000);
    assert!(m.blocking.allow_ready(NOW + 5_100));
    m.step_blocking(true, NOW + 6_000);
    assert!(!m.blocking.allow_ready(NOW + 6_500));
}

#[test]
fn a_calm_open_closes_after_its_time_and_an_urgent_one_stays() {
    let mut m = model_with(vec![item("a", None)]);
    m.open_blocking("a", 8_000, false, NOW);
    assert_eq!(m.state, NotchState::Blocking);
    m.tick(NOW + 7_999);
    assert_eq!(m.state, NotchState::Blocking);
    assert!(m.tick(NOW + 8_000));
    assert_eq!(m.state, NotchState::Collapsed);
    m.open_blocking("a", 8_000, true, NOW + 9_000);
    m.tick(NOW + 600_000);
    assert_eq!(m.state, NotchState::Blocking);
    m.close_blocking();
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn a_grow_lasts_four_seconds_and_only_starts_from_closed() {
    let mut m = model_with(vec![item("a", None)]);
    m.grow_blocking("a", NOW);
    assert_eq!(m.state, NotchState::Glance);
    assert!(m.tick(NOW + 4_000));
    assert_eq!(m.state, NotchState::Collapsed);
    m.open_blocking("a", 8_000, true, NOW);
    m.grow_blocking("a", NOW);
    assert_eq!(m.state, NotchState::Blocking);
}

#[test]
fn an_unknown_key_opens_nothing() {
    let mut m = model_with(vec![item("a", None)]);
    m.open_blocking("zzz", 8_000, true, NOW);
    m.grow_blocking("zzz", NOW);
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn answered_shows_two_seconds_then_the_notch_closes() {
    let mut m = model_with(vec![item("a", None)]);
    m.open_blocking("a", 8_000, true, NOW);
    m.set_inbox_items(&[], NOW + 3_000);
    let farewell = m.blocking.farewell_live(NOW + 3_000).unwrap();
    assert_eq!(farewell.kind, Closing::Answered);
    assert_eq!(m.state, NotchState::Blocking);
    m.tick(NOW + 4_999);
    assert_eq!(m.state, NotchState::Blocking);
    assert!(m.tick(NOW + 5_000));
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn expired_shows_four_seconds() {
    let mut m = model_with(vec![item("a", None)]);
    m.open_blocking("a", 8_000, true, NOW);
    let mut gone = item("a", None);
    gone.state = State::Expired;
    m.set_inbox_items(&[gone], NOW + 3_000);
    let farewell = m.blocking.farewell_live(NOW + 6_999).unwrap();
    assert_eq!(farewell.kind, Closing::Expired);
    assert!(m.tick(NOW + 7_000));
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn the_open_height_is_capped_at_45_percent_of_the_screen() {
    let mut long = item("a", None);
    long.detail = "word ".repeat(300);
    let mut m = model_with(vec![long]);
    m.screen_h = 400.0;
    assert_eq!(blocking_cap(400.0), 180.0);
    assert_eq!(m.size(&NotchState::Blocking).h, 180.0);
    m.screen_h = 1600.0;
    assert!(m.size(&NotchState::Blocking).h > 180.0);
}

#[test]
fn the_closed_notch_grows_ears_only_while_something_blocks() {
    let calm = NotchModel::default();
    let mut m = model_with(vec![item("a", None)]);
    assert!(m.size(&NotchState::Collapsed).w > calm.size(&NotchState::Collapsed).w);
    m.set_inbox_items(&[], NOW);
    assert_eq!(
        m.size(&NotchState::Collapsed),
        calm.size(&NotchState::Collapsed)
    );
}

#[test]
fn hiding_the_notch_closes_any_blocking_state() {
    let mut m = model_with(vec![item("a", None)]);
    m.open_blocking("a", 8_000, true, NOW);
    m.apply(&NotchPrefs {
        shown: false,
        ..NotchPrefs::default()
    });
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn the_effects_open_grow_and_close_the_notch() {
    use crate::attention::{BlockingTarget, Effect};
    let key = BlockingTarget::Item("a".into());
    let mut m = model_with(vec![item("a", None)]);
    m.apply_effect(&Effect::GrowNotch { key: key.clone() }, NOW);
    assert_eq!(m.state, NotchState::Glance);
    m.apply_effect(
        &Effect::OpenNotch {
            key: key.clone(),
            ms: 45_000,
            stay: true,
        },
        NOW,
    );
    assert_eq!(m.state, NotchState::Blocking);
    m.tick(NOW + 100_000);
    assert_eq!(m.state, NotchState::Blocking);
    m.apply_effect(&Effect::CloseNotch, NOW + 100_000);
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn close_notch_waits_for_the_answer_shown_to_end() {
    use crate::attention::{BlockingTarget, Effect};
    let mut m = model_with(vec![item("a", None), item("b", Some(30))]);
    m.apply_effect(
        &Effect::OpenNotch {
            key: BlockingTarget::Item("b".into()),
            ms: 45_000,
            stay: true,
        },
        NOW,
    );
    m.set_inbox_items(&[item("a", None)], NOW + 1_000);
    m.apply_effect(&Effect::CloseNotch, NOW + 1_000);
    assert_eq!(m.state, NotchState::Blocking);
    m.tick(NOW + 2_999);
    assert_eq!(m.state, NotchState::Blocking);
    assert!(m.tick(NOW + 3_000));
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn a_hook_prompt_effect_opens_by_its_request_id() {
    use crate::attention::{BlockingTarget, Effect};
    let mut m = model_with(Vec::new());
    m.requests.insert("s9".into(), hook("r1"));
    m.rebuild_blocking(NOW);
    m.apply_effect(
        &Effect::OpenNotch {
            key: BlockingTarget::HookPrompt("r1".into()),
            ms: 45_000,
            stay: false,
        },
        NOW,
    );
    assert_eq!(m.state, NotchState::Blocking);
}
