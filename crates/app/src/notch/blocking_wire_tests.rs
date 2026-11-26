use notchling_engine::inbox::{Action, ActionId, AlertAction, Item, Kind, State};
use notchling_locale as t;
use serde_json::json;

use super::super::blocking::{
    allow_route, button_label, card_of_hook, card_of_item, item_buttons, step_label, AllowRoute,
    Button, Closing, NoteRoute,
};
use super::super::layout::{
    blocked_size, blocking_card_height, collapsed_size, CardDims, NotchState,
};
use super::super::model::NotchModel;
use super::super::prefs::NotchPrefs;
use super::super::requests::{HookRequest, RequestKind};
use super::ClosedCount;
use crate::attention::{Attention, BlockingTarget, Effect, Input, Level, Notifications, Target};

const NOW: i64 = 1_000_000_000;

fn consent(id: &str, kind: &str, alerts: Vec<AlertAction>) -> Item {
    Item {
        id: id.to_owned(),
        kind: Kind::Consent,
        title: format!("Title {id}"),
        state: State::Open,
        created_ms: NOW - 180_000,
        blocking: Some(true),
        waiting_since: Some(NOW - 120_000),
        expires_ms: Some(NOW + 300_000),
        alert_actions: alerts,
        open: Some(json!({"consent": format!("c-{id}"), "kind": kind})),
        ..Item::default()
    }
}

fn allow_deny_change() -> Vec<AlertAction> {
    vec![AlertAction::Allow, AlertAction::Deny, AlertAction::Change]
}

fn deny_open_change() -> Vec<AlertAction> {
    vec![AlertAction::Deny, AlertAction::Open, AlertAction::Change]
}

fn approval(id: &str) -> Item {
    Item {
        kind: Kind::SecretApproval,
        alert_actions: deny_open_change(),
        actions: vec![
            Action {
                id: ActionId::AllowOnce,
                label: "Allow Once".into(),
                primary: true,
            },
            Action {
                id: ActionId::Deny,
                label: "Deny".into(),
                primary: false,
            },
        ],
        open: Some(json!({"vault_approval": "ap-1"})),
        ..consent(id, "none", Vec::new())
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
        can_always_allow: true,
        agy: None,
    }
}

fn model_with(items: Vec<Item>) -> NotchModel {
    let mut model = NotchModel::default();
    model.now_ms = NOW;
    model.set_inbox_items(&items, NOW);
    model
}

fn open(id: &str) -> Effect {
    Effect::OpenNotch {
        key: BlockingTarget::Item(id.to_owned()),
        ms: 45_000,
        stay: true,
    }
}

fn input(level: Level, id: &str) -> Input {
    Input {
        targets: vec![Target {
            key: BlockingTarget::Item(id.to_owned()),
            title: "Turn On Machines".to_owned(),
            waiting_since_ms: NOW,
            expires_ms: Some(NOW + 300_000),
            alert_actions: allow_deny_change(),
        }],
        level,
        notch_shown: true,
        notifications: Notifications::Allowed,
        reduce_motion: false,
        app_active: false,
        delivered: Vec::new(),
        closed: Vec::new(),
        expired: Vec::new(),
    }
}

#[test]
fn a_steps_effects_reach_the_notch_model() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    let mut attention = Attention::new();
    let effects = attention.step(&input(Level::Normal, "a"), NOW);
    m.apply_effects(&effects, NOW);
    assert_eq!(m.state, NotchState::Blocking);
    assert_eq!(m.blocking.current().map(|c| c.id.as_str()), Some("a"));
}

#[test]
fn effects_that_are_not_for_the_notch_change_nothing() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    m.apply_effects(&[Effect::Tone { times: 1 }, Effect::PulseTray], NOW);
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn an_urgent_start_through_the_model_stays_open() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    let mut attention = Attention::new();
    let effects = attention.step(&input(Level::Urgent, "a"), NOW);
    m.apply_effects(&effects, NOW);
    m.tick(NOW + 600_000);
    assert_eq!(m.state, NotchState::Blocking);
}

#[test]
fn secret_use_offers_allow_once_deny_change_and_later() {
    let it = approval("p");
    assert_eq!(
        item_buttons(&it, true),
        [
            Button::AllowOnce,
            Button::Deny,
            Button::Change,
            Button::Later
        ]
    );
    assert_eq!(
        item_buttons(&it, false),
        [Button::Open, Button::Deny, Button::Change, Button::Later]
    );
}

#[test]
fn allow_comes_from_the_kind_not_the_banner_list() {
    for kind in [
        "session",
        "dispatch",
        "job",
        "switch",
        "setting",
        "session_stop",
        "job_stop",
    ] {
        let it = consent("k", kind, deny_open_change());
        assert_eq!(
            item_buttons(&it, true),
            [Button::Allow, Button::Deny, Button::Change, Button::Later],
            "{kind}"
        );
        assert!(allow_route(&it).is_some(), "{kind}");
    }
    for kind in [
        "model",
        "update",
        "account",
        "machine",
        "install",
        "cost_profile",
        "cluster",
    ] {
        let it = consent("k", kind, deny_open_change());
        assert_eq!(
            item_buttons(&it, true),
            [Button::Deny, Button::Open, Button::Change, Button::Later],
            "{kind}"
        );
        assert_eq!(allow_route(&it), None, "{kind}");
    }
}

#[test]
fn an_unverified_caller_gets_deny_and_open_whatever_the_kind() {
    let it = consent("u", "session", vec![AlertAction::Deny, AlertAction::Open]);
    assert_eq!(
        item_buttons(&it, true),
        [Button::Deny, Button::Open, Button::Later]
    );
    assert_eq!(allow_route(&it), None);
}

#[test]
fn allow_routes_follow_the_item() {
    let session = consent("s", "session", deny_open_change());
    assert_eq!(
        allow_route(&session),
        Some(AllowRoute::Consent("c-s".into()))
    );
    let mut switch = consent("w", "switch", allow_deny_change());
    switch.switch = Some("machines".into());
    assert_eq!(
        allow_route(&switch),
        Some(AllowRoute::Switch {
            consent: "c-w".into(),
            switch: "machines".into()
        })
    );
    assert_eq!(
        allow_route(&approval("p")),
        Some(AllowRoute::ApprovalOnce("ap-1".into()))
    );
}

#[test]
fn a_change_note_goes_where_the_card_answers() {
    let words = |_: &Item| (String::new(), String::new());
    let _ = words;
    let consent_card = card_of_item(
        &consent("a", "setting", allow_deny_change()),
        true,
        String::new(),
        String::new(),
    );
    assert_eq!(
        consent_card.note_route,
        Some(NoteRoute::Consent("c-a".into()))
    );
    let approval_card = card_of_item(&approval("p"), true, String::new(), String::new());
    assert_eq!(
        approval_card.note_route,
        Some(NoteRoute::Approval("ap-1".into()))
    );
    let hook_card = card_of_hook(&hook("r1"), true).unwrap();
    assert_eq!(hook_card.note_route, Some(NoteRoute::Hook("r1".into())));
    let mut form = consent("f", "secret_request", Vec::new());
    form.alert_actions = vec![
        AlertAction::OpenForm,
        AlertAction::Cancel,
        AlertAction::Change,
    ];
    let form_card = card_of_item(&form, true, String::new(), String::new());
    assert_eq!(form_card.note_route, None);
}

#[test]
fn the_note_field_opens_for_a_card_and_closes_when_the_card_changes() {
    let mut m = model_with(vec![
        consent("a", "setting", allow_deny_change()),
        consent("b", "setting", allow_deny_change()),
    ]);
    m.blocking.open_note("a");
    m.blocking.set_draft("a", "use staging");
    assert_eq!(m.blocking.note_open(), Some("a"));
    m.step_blocking(true, NOW + 1_000);
    assert_eq!(m.blocking.note_open(), None);
    assert_eq!(m.blocking.draft("a"), "use staging");
}

#[test]
fn sending_a_note_reads_the_draft_cut_at_300_characters() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    m.blocking.open_note("a");
    assert_eq!(m.blocking.note_to_send("a"), None);
    m.blocking.set_draft("a", &"x".repeat(400));
    let (route, note) = m.blocking.note_to_send("a").unwrap();
    assert_eq!(route, NoteRoute::Consent("c-a".into()));
    assert_eq!(note.chars().count(), 300);
    assert_eq!(m.blocking.draft("a").chars().count(), 300);
    assert_eq!(m.blocking.note_open(), Some("a"));
}

#[test]
fn a_note_is_cleared_only_after_the_deny_succeeds() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    m.blocking.open_note("a");
    m.blocking.set_draft("a", "use staging");
    m.blocking.note_settled("a", false);
    assert_eq!(m.blocking.draft("a"), "use staging");
    assert_eq!(m.blocking.note_open(), Some("a"));
    assert_eq!(
        m.blocking.note_to_send("a").map(|(_, note)| note),
        Some("use staging".to_owned())
    );
    m.blocking.note_settled("a", true);
    assert_eq!(m.blocking.draft("a"), "");
    assert_eq!(m.blocking.note_open(), None);
    assert_eq!(m.blocking.note_to_send("a"), None);
}

#[test]
fn a_failed_send_reopens_a_note_the_card_had_closed() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    m.blocking.set_draft("a", "use staging");
    m.blocking.close_note();
    m.blocking.note_settled("a", false);
    assert_eq!(m.blocking.note_open(), Some("a"));
    assert_eq!(m.blocking.draft("a"), "use staging");
}

#[test]
fn a_hook_prompt_has_the_guard_on_allow_only() {
    let mut m = model_with(Vec::new());
    m.requests.insert("s9".into(), hook("r1"));
    m.rebuild_blocking(NOW);
    let card = m.blocking.current().unwrap().clone();
    assert_eq!(
        card.buttons,
        [Button::Allow, Button::Deny, Button::Change, Button::Later]
    );
    assert!(!m.blocking.ready(Button::Allow, NOW + 500));
    assert!(m.blocking.ready(Button::Allow, NOW + 1_000));
    for button in [Button::Deny, Button::Change, Button::Later, Button::Open] {
        assert!(m.blocking.ready(button, NOW));
    }
    assert!(!m.blocking.ready(Button::AllowOnce, NOW + 500));
}

#[test]
fn a_hook_prompt_with_answering_off_shows_open_and_still_denies() {
    let mut m = model_with(Vec::new());
    m.requests.insert("s9".into(), hook("r1"));
    m.apply(&NotchPrefs {
        answer: false,
        ..NotchPrefs::default()
    });
    let card = m.blocking.current().unwrap();
    assert_eq!(
        card.buttons,
        [Button::Open, Button::Deny, Button::Change, Button::Later]
    );
}

#[test]
fn a_hook_height_counts_its_summary_and_every_button_row() {
    let mut m = model_with(Vec::new());
    let mut long = hook("r1");
    long.summary = "x".repeat(300);
    m.requests.insert("s9".into(), long.clone());
    m.rebuild_blocking(NOW);
    let want = blocking_card_height(CardDims {
        detail_chars: 300,
        title_chars: m.blocking.current().unwrap().title.chars().count(),
        last_call: false,
        note: false,
        mono: true,
    });
    assert_eq!(m.blocking_content_height(), want);
    m.requests.insert("s9".into(), hook("r1"));
    m.rebuild_blocking(NOW);
    assert!(m.blocking_content_height() < want);
}

#[test]
fn the_answered_farewell_names_the_answer() {
    let cases = [
        (Some(ActionId::Allow), None, Closing::Allowed),
        (Some(ActionId::AllowOnce), None, Closing::Allowed),
        (Some(ActionId::Deny), None, Closing::Denied),
        (Some(ActionId::Deny), Some("use staging"), Closing::Changed),
        (None, None, Closing::Answered),
    ];
    for (answer, note, want) in cases {
        let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
        m.open_blocking("a", 8_000, true, NOW);
        let mut done = consent("a", "setting", allow_deny_change());
        done.state = State::Answered;
        done.answer = answer;
        done.note = note.map(str::to_owned);
        m.set_inbox_items(&[done], NOW + 1_000);
        assert_eq!(m.blocking.farewell_live(NOW + 1_500).unwrap().kind, want);
    }
}

#[test]
fn a_hook_answer_made_here_is_named_when_it_leaves() {
    let mut m = model_with(Vec::new());
    m.requests.insert("s9".into(), hook("r1"));
    m.rebuild_blocking(NOW);
    m.blocking.note_answer("r1", Closing::Denied);
    m.answered.insert("r1".into());
    m.rebuild_blocking(NOW + 1_000);
    assert_eq!(
        m.blocking.farewell_live(NOW + 1_500).unwrap().kind,
        Closing::Denied
    );
}

#[test]
fn the_farewell_words_say_allowed_denied_or_changed() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    m.open_blocking("a", 8_000, true, NOW);
    let mut done = consent("a", "setting", allow_deny_change());
    done.state = State::Answered;
    done.answer = Some(ActionId::Allow);
    m.set_inbox_items(&[done], NOW + 1_000);
    m.now_ms = NOW + 1_500;
    let (title, word) = m.shown_farewell().unwrap();
    assert_eq!(title, "Title a");
    assert_eq!(word, t::consent::notch_allowed());
}

#[test]
fn leaving_during_a_glance_shows_the_farewell_not_a_blank_bar() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    m.grow_blocking("a", NOW);
    assert_eq!(m.state, NotchState::Glance);
    m.set_inbox_items(&[], NOW + 1_000);
    m.now_ms = NOW + 1_200;
    assert_eq!(m.state, NotchState::Glance);
    assert!(m.shown_farewell().is_some());
}

#[test]
fn the_red_glow_is_for_normal_and_urgent_only() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    assert!(!m.glow());
    m.apply_effect(&open("a"), NOW);
    for (level, glow) in [
        (Level::Calm, false),
        (Level::Normal, true),
        (Level::Urgent, true),
    ] {
        m.apply(&NotchPrefs {
            level,
            ..NotchPrefs::default()
        });
        assert_eq!(m.glow(), glow, "{level:?}");
    }
}

#[test]
fn reduce_motion_stops_the_notch_animating() {
    let mut m = NotchModel::default();
    assert!(m.animates());
    m.reduce_motion = true;
    assert!(!m.animates());
}

#[test]
fn waiting_items_count_orange_on_the_closed_notch_only_when_none_blocks() {
    let mut waiting = consent("w", "setting", allow_deny_change());
    waiting.blocking = None;
    let mut m = model_with(vec![waiting.clone()]);
    assert_eq!(
        m.closed_count(),
        Some(ClosedCount {
            count: 1,
            blocking: false
        })
    );
    assert_eq!(m.size(&NotchState::Collapsed), blocked_size(m.notch));
    m.set_inbox_items(
        &[
            waiting.clone(),
            consent("b", "setting", allow_deny_change()),
        ],
        NOW,
    );
    assert_eq!(
        m.closed_count(),
        Some(ClosedCount {
            count: 1,
            blocking: true
        })
    );
    m.set_inbox_items(&[], NOW);
    assert_eq!(m.closed_count(), None);
    assert_eq!(m.size(&NotchState::Collapsed), collapsed_size(m.notch));
}

#[test]
fn the_card_height_grows_for_the_last_call_clock_a_second_title_line_and_a_note() {
    let base = CardDims {
        detail_chars: 0,
        title_chars: 20,
        last_call: false,
        note: false,
        mono: false,
    };
    let plain = blocking_card_height(base);
    assert!(
        blocking_card_height(CardDims {
            last_call: true,
            ..base
        }) >= plain + 26.0
    );
    assert!(
        blocking_card_height(CardDims {
            title_chars: 90,
            ..base
        }) >= plain + 16.0
    );
    assert!(blocking_card_height(CardDims { note: true, ..base }) >= plain + 42.0);
}

#[test]
fn the_last_call_card_is_taller_than_the_same_card_earlier() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    let early = m.blocking_content_height();
    m.now_ms = NOW + 250_000;
    assert!(m.blocking_content_height() > early);
}

#[test]
fn buttons_and_the_stepper_have_spoken_labels() {
    let card = card_of_item(
        &consent("a", "setting", allow_deny_change()),
        true,
        "Turn on Machines".into(),
        String::new(),
    );
    assert_eq!(button_label(&card, Button::Allow), "Allow Turn on Machines");
    assert_eq!(step_label(true, 0, 3), "Next blocked task, 2 of 3");
    assert_eq!(step_label(false, 0, 3), "Previous blocked task, 3 of 3");
}

#[test]
fn a_denial_with_a_note_for_a_held_prompt_is_one_permission_call() {
    assert_eq!(
        crate::acting::Request::permission_deny_with("r1", "use staging").args,
        json!({"id": "r1", "decision": "deny", "message": "use staging"})
    );
}

#[test]
fn a_second_answer_for_the_same_item_is_refused_while_the_first_is_in_flight() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    assert!(!m.blocking.busy("a"));
    assert!(m.blocking.begin("a"));
    assert!(m.blocking.busy("a"));
    assert!(!m.blocking.begin("a"));
}

#[test]
fn a_busy_item_disables_every_answer_but_keeps_later_and_open() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    for b in [
        Button::Allow,
        Button::AllowOnce,
        Button::Deny,
        Button::Change,
    ] {
        assert!(m.blocking.pressable("a", b, NOW + 5_000));
    }
    m.blocking.begin("a");
    for b in [
        Button::Allow,
        Button::AllowOnce,
        Button::Deny,
        Button::Change,
    ] {
        assert!(!m.blocking.pressable("a", b, NOW + 5_000));
    }
    for b in [Button::Later, Button::Open, Button::OpenForm] {
        assert!(m.blocking.pressable("a", b, NOW + 5_000));
    }
}

#[test]
fn a_failed_answer_frees_the_item_and_a_sent_one_holds_it_until_it_leaves() {
    let mut m = model_with(vec![
        consent("a", "setting", allow_deny_change()),
        consent("b", "setting", allow_deny_change()),
    ]);
    m.blocking.begin("a");
    m.blocking.finish("a", false);
    assert!(!m.blocking.busy("a"));
    m.blocking.begin("a");
    m.blocking.begin("b");
    m.blocking.finish("a", true);
    assert!(m.blocking.busy("a"));
    m.set_inbox_items(&[consent("b", "setting", allow_deny_change())], NOW + 1_000);
    assert!(!m.blocking.busy("a"));
    assert!(m.blocking.busy("b"));
}

#[test]
fn a_hook_prompt_answer_holds_its_card_until_the_card_leaves() {
    let mut m = model_with(Vec::new());
    m.requests.insert("s9".into(), hook("r1"));
    m.rebuild_blocking(NOW);
    assert!(m.blocking.begin("r1"));
    assert!(!m.blocking.pressable("r1", Button::Deny, NOW + 5_000));
    m.requests.clear();
    m.rebuild_blocking(NOW + 1_000);
    assert!(!m.blocking.busy("r1"));
}

#[test]
fn a_note_in_flight_blocks_a_second_answer_and_a_hook_answer_leaves_it_alone() {
    let mut m = model_with(vec![consent("a", "setting", allow_deny_change())]);
    m.requests.insert("s9".into(), hook("r1"));
    m.rebuild_blocking(NOW);
    m.blocking.open_note("a");
    m.blocking.set_draft("a", "use staging");
    assert!(m.blocking.begin("a"));
    assert!(m.blocking.begin("r1"));
    m.blocking.finish("r1", true);
    assert_eq!(m.blocking.draft("a"), "use staging");
    assert!(m.blocking.busy("a"));
    assert!(!m.blocking.pressable("a", Button::Deny, NOW + 5_000));
    m.blocking.note_settled("a", false);
    m.blocking.finish("a", false);
    assert_eq!(m.blocking.draft("a"), "use staging");
    assert!(m.blocking.pressable("a", Button::Deny, NOW + 5_000));
}
