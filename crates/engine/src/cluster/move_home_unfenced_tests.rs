use super::move_home::{
    next, unfenced_notice, MoveAction, MoveEvent, MoveSaga, MoveStep, SwitchResult,
};
use super::testkit::id;

fn at_switch() -> MoveSaga {
    let mut saga = MoveSaga::new(id("m5"), id("m1"), 1_000, false);
    saga.step = MoveStep::Switch;
    saga.moving = vec!["a".to_string(), "c".to_string()];
    saga.to_name = "m1".to_string();
    saga
}

#[test]
fn a_switch_that_could_not_pause_the_old_copies_goes_on_to_resume_and_says_so() {
    let (after, actions) = next(
        &at_switch(),
        MoveEvent::Switched(SwitchResult::DoneUnfenced("fence.json: busy".to_string())),
        true,
    );
    assert_eq!(after.step, MoveStep::Resume);
    assert_eq!(after.unfenced.as_deref(), Some("fence.json: busy"));
    assert_eq!(
        actions,
        vec![MoveAction::Resume {
            sessions: vec!["a".to_string(), "c".to_string()]
        }]
    );
}

#[test]
fn a_clean_switch_leaves_nothing_unfenced() {
    let (after, _) = next(&at_switch(), MoveEvent::Switched(SwitchResult::Done), true);
    assert_eq!(after.step, MoveStep::Resume);
    assert_eq!(after.unfenced, None);
}

#[test]
fn the_unfenced_note_survives_the_journal_and_an_old_journal_still_reads() {
    let (after, _) = next(
        &at_switch(),
        MoveEvent::Switched(SwitchResult::DoneUnfenced("why".to_string())),
        true,
    );
    let text = serde_json::to_string(&after).unwrap();
    let back: MoveSaga = serde_json::from_str(&text).unwrap();
    assert_eq!(back, after);
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value.as_object_mut().unwrap().remove("unfenced");
    let old: MoveSaga = serde_json::from_value(value).unwrap();
    assert_eq!(old.unfenced, None);
}

#[test]
fn the_notice_names_the_sessions_and_says_they_may_run_on_both_machines() {
    let (title, detail) = unfenced_notice(&["a".to_string(), "c".to_string()], "fence.json: busy");
    assert!(title.contains("a, c"), "{title}");
    assert!(detail.contains("fence.json: busy"), "{detail}");
    assert!(detail.contains("both machines"), "{detail}");
    let (_, one) = unfenced_notice(&["a".to_string()], "why");
    assert!(one.contains("It may run on both machines"), "{one}");
}
