use super::*;
use crate::attention::{Effect, Level};
use crate::inbox::driver::{Driver, Surroundings};
use notchling_engine::inbox::{Action, ActionId, Kind, Place};
use notchling_platform::notify::RecordingNotifier;

const T0: i64 = 5_000_000;

fn around() -> Surroundings {
    Surroundings {
        level: Level::Normal,
        notch_shown: true,
        reduce_motion: false,
        app_active: false,
    }
}

fn item(id: &str, blocking: bool, alerts: &[AlertAction]) -> Item {
    Item {
        id: id.to_owned(),
        kind: Kind::default(),
        title: format!("Title {id}"),
        detail: format!("Detail {id}"),
        created_ms: T0,
        actions: vec![Action {
            id: ActionId::Deny,
            label: "Deny".to_owned(),
            primary: false,
        }],
        state: State::Open,
        blocking: Some(blocking),
        waiting_since: Some(T0),
        alert_actions: alerts.to_vec(),
        place: Some(Place {
            session_name: Some("notchling".to_owned()),
            session: Some("s1".to_owned()),
            ..Place::default()
        }),
        ..Item::default()
    }
}

fn model_of(items: Vec<Item>) -> InboxModel {
    InboxModel::from_items(items, T0)
}

const ALLOW: [AlertAction; 3] = [AlertAction::Allow, AlertAction::Deny, AlertAction::Change];
const MODEL: [AlertAction; 3] = [AlertAction::Deny, AlertAction::Open, AlertAction::Change];

#[test]
fn a_blocking_item_posts_one_banner_with_the_allow_category_and_the_sound() {
    let notifier = RecordingNotifier::new();
    let mut driver = Driver::new();
    driver.run(
        &model_of(vec![item("a", true, &ALLOW)]),
        &around(),
        &notifier,
        T0,
    );
    let posted = notifier.upserts();
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0].0, "inbox:a");
    assert_eq!(posted[0].1.title, "Title a");
    assert_eq!(posted[0].1.category, Some(Category::InboxAllow));
    assert_eq!(posted[0].1.sound.as_deref(), Some(SOUND));
    assert_eq!(posted[0].1.thread.as_deref(), Some("s1"));
}

#[test]
fn a_non_blocking_item_posts_nothing_at_any_level() {
    for level in [Level::Calm, Level::Normal, Level::Urgent] {
        let notifier = RecordingNotifier::new();
        let mut driver = Driver::new();
        let mut around = around();
        around.level = level;
        let model = model_of(vec![item("a", false, &ALLOW)]);
        for at in [0, 60_000, 180_000, 600_000] {
            driver.run(&model, &around, &notifier, T0 + at);
        }
        assert!(notifier.upserts().is_empty());
    }
}

#[test]
fn calm_posts_a_silent_banner() {
    let notifier = RecordingNotifier::new();
    let mut driver = Driver::new();
    let mut around = around();
    around.level = Level::Calm;
    driver.run(
        &model_of(vec![item("a", true, &ALLOW)]),
        &around,
        &notifier,
        T0,
    );
    assert_eq!(notifier.upserts()[0].1.sound, None);
}

#[test]
fn the_pulse_reposts_the_same_id_and_a_close_removes_it() {
    let notifier = RecordingNotifier::new();
    let mut driver = Driver::new();
    let open = model_of(vec![item("a", true, &ALLOW)]);
    driver.run(&open, &around(), &notifier, T0);
    driver.run(&open, &around(), &notifier, T0 + 60_000);
    let posted = notifier.upserts();
    assert_eq!(posted.len(), 2);
    assert_eq!(posted[1].0, "inbox:a");
    assert_eq!(posted[1].1.body, "Stopped 1m");
    assert_eq!(notifier.delivered(), ["inbox:a"]);
    driver.run(&model_of(Vec::new()), &around(), &notifier, T0 + 70_000);
    assert_eq!(notifier.removed(), ["inbox:a"]);
    assert!(notifier.delivered().is_empty());
}

#[test]
fn denied_notifications_post_nothing_and_leave_the_rest_to_the_notch() {
    let notifier = RecordingNotifier::new();
    notifier.deny();
    let mut driver = Driver::new();
    let ui = driver.run(
        &model_of(vec![item("a", true, &ALLOW)]),
        &around(),
        &notifier,
        T0,
    );
    assert!(notifier.upserts().is_empty());
    assert!(ui.iter().any(|e| matches!(e, Effect::OpenNotch { .. })));
    assert!(ui
        .iter()
        .any(|e| matches!(e, Effect::Warn(crate::attention::Warning::NoChannel))));
}

#[test]
fn an_allow_half_a_second_after_the_banner_opens_the_card_and_answers_nothing() {
    let notifier = RecordingNotifier::new();
    let mut driver = Driver::new();
    driver.run(
        &model_of(vec![item("a", true, &ALLOW)]),
        &around(),
        &notifier,
        T0,
    );
    let response = notifier
        .simulate_action("inbox:a", BannerAction::Allow)
        .expect("the banner offers Allow");
    assert_eq!(
        driver.route(&response, T0 + 500),
        Route::OpenInbox("a".to_owned())
    );
    assert_eq!(
        driver.route(&response, T0 + 1_500),
        Route::Act(BlockAct::Allow, "a".to_owned())
    );
}

#[test]
fn deny_answers_with_a_deny() {
    let notifier = RecordingNotifier::new();
    let mut driver = Driver::new();
    driver.run(
        &model_of(vec![item("a", true, &ALLOW)]),
        &around(),
        &notifier,
        T0,
    );
    let response = notifier
        .simulate_action("inbox:a", BannerAction::Deny)
        .expect("the banner offers Deny");
    assert_eq!(
        driver.route(&response, T0 + 200),
        Route::Act(BlockAct::Deny, "a".to_owned())
    );
}

#[test]
fn a_model_item_never_offers_allow() {
    let notifier = RecordingNotifier::new();
    let mut driver = Driver::new();
    driver.run(
        &model_of(vec![item("m", true, &MODEL)]),
        &around(),
        &notifier,
        T0,
    );
    assert_eq!(notifier.upserts()[0].1.category, Some(Category::InboxOpen));
    assert!(notifier
        .simulate_action("inbox:m", BannerAction::Allow)
        .is_none());
    assert!(notifier
        .simulate_action("inbox:m", BannerAction::Open)
        .is_some());
}

#[test]
fn a_click_and_the_open_buttons_open_the_card() {
    for action in [
        None,
        Some(BannerAction::Open),
        Some(BannerAction::Change),
        Some(BannerAction::Reply),
        Some(BannerAction::OpenForm),
    ] {
        let response = Response {
            id: "inbox:a".to_owned(),
            action,
        };
        assert_eq!(route(&response, None, T0), Route::OpenInbox("a".to_owned()));
    }
    let hook = Response {
        id: "hook:p".to_owned(),
        action: None,
    };
    assert_eq!(route(&hook, None, T0), Route::Ignore);
}

#[test]
fn categories_follow_the_alert_actions() {
    assert_eq!(category_for(&ALLOW), Some(Category::InboxAllow));
    assert_eq!(category_for(&MODEL), Some(Category::InboxOpen));
    assert_eq!(
        category_for(&[AlertAction::Deny, AlertAction::Open]),
        Some(Category::InboxOpenOnly)
    );
    assert_eq!(
        category_for(&[AlertAction::OpenForm, AlertAction::Cancel]),
        Some(Category::SecretRequest)
    );
    assert_eq!(
        category_for(&[AlertAction::Reply, AlertAction::Dismiss]),
        Some(Category::Ask)
    );
    assert_eq!(category_for(&[]), None);
}

#[test]
fn banner_action_titles_are_short() {
    for category in Category::ALL {
        for action in category.actions() {
            assert!(action_title(*action).chars().count() <= 20);
        }
    }
}

#[test]
fn ids_round_trip() {
    for key in [
        BlockingTarget::Item("x".to_owned()),
        BlockingTarget::HookPrompt("y".to_owned()),
    ] {
        assert_eq!(key_of(&banner_id(&key)), Some(key));
    }
}

fn closed_copy(id: &str, state: State) -> Item {
    Item {
        state,
        ..item(id, true, &ALLOW)
    }
}

#[test]
fn only_an_open_item_can_be_answered_from_a_banner() {
    assert!(answerable(&closed_copy("a", State::Open)));
    assert!(!answerable(&closed_copy("a", State::Answered)));
    assert!(!answerable(&closed_copy("a", State::Expired)));
    assert!(!answerable(&closed_copy("a", State::Withdrawn)));
}

#[test]
fn a_refused_allow_is_an_error_or_a_false_answer() {
    assert!(!allow_refused(&Ok(true)));
    assert!(allow_refused(&Ok(false)));
    assert!(allow_refused(&Err("refused".to_owned())));
}

fn drive(model: &InboxModel, driver: &mut Driver, notifier: &RecordingNotifier, at: i64) {
    driver.run(model, &around(), notifier, at);
}

#[test]
fn an_answered_item_removes_its_banner_and_posts_no_expired_one_even_past_its_end() {
    let notifier = RecordingNotifier::new();
    let mut driver = Driver::new();
    let mut open = item("a", true, &ALLOW);
    open.expires_ms = Some(T0 + 300_000);
    drive(&model_of(vec![open.clone()]), &mut driver, &notifier, T0);
    assert_eq!(notifier.delivered(), ["inbox:a"]);
    let answered = Item {
        state: State::Answered,
        ..open
    };
    drive(
        &model_of(vec![answered]),
        &mut driver,
        &notifier,
        T0 + 400_000,
    );
    assert_eq!(notifier.removed(), ["inbox:a"]);
    assert_eq!(notifier.upserts().len(), 1);
    assert!(notifier.delivered().is_empty());
}

#[test]
fn an_expired_item_posts_the_expired_banner() {
    let notifier = RecordingNotifier::new();
    let mut driver = Driver::new();
    let mut open = item("a", true, &ALLOW);
    open.expires_ms = Some(T0 + 300_000);
    drive(&model_of(vec![open.clone()]), &mut driver, &notifier, T0);
    let expired = Item {
        state: State::Expired,
        ..open
    };
    drive(
        &model_of(vec![expired]),
        &mut driver,
        &notifier,
        T0 + 301_000,
    );
    let posted = notifier.upserts();
    assert_eq!(posted.len(), 2);
    assert_eq!(posted[1].1.title, "Expired: Title a");
    assert!(posted[1].1.sound.is_none());
}

#[test]
fn the_banner_worker_is_ready_only_after_its_first_refresh() {
    let notifier = Arc::new(RecordingNotifier::new());
    let worker = Detached::new(notifier);
    assert!(!worker.ready());
    worker.refresh();
    let mut waited = 0;
    while !worker.ready() && waited < 2_000 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        waited += 5;
    }
    assert!(worker.ready());
}
