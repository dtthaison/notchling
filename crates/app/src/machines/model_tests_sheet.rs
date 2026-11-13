use notchling_engine::machines::engine_plan::PlanKind;
use notchling_engine::machines::step::StepId;

use super::forms::AddForm;
use super::model::Sheet;
use super::model_tests::{fix_sheet, mac, off_world, with_job};

#[test]
fn no_other_fix_starts_while_a_sheet_is_open() {
    let mut w = off_world(vec![with_job(mac("m1")), mac("m2")]);
    w.model.engine().vault_value_changed("claude-worker");
    w.model.request_set_up(&StepId::BASE_TOOLS, "m1");
    let shown = fix_sheet(&w.model).clone();
    assert_eq!(shown.plan.kind, PlanKind::Fix(StepId::BASE_TOOLS));

    w.model.request_set_up(&StepId::STAYS_AWAKE, "m2");
    w.model.request_replace_key(&StepId::BASE_TOOLS, "m1");
    w.model.request_set_up_all("m2");
    w.model.request_toggle("m1", false);
    w.model.request_update("claude-worker");
    for usage in notchling_engine::machines::steps_secret::usages(&w.model.machines) {
        w.model.request_send_again(&usage);
    }
    assert_eq!(fix_sheet(&w.model), &shown, "the sheet on show stays");
    assert!(!w.model.open_sheet(Sheet::Add(AddForm::new())));
    assert_eq!(fix_sheet(&w.model), &shown);
    assert!(
        w.model.outcomes.is_empty() && w.model.update_notes.is_empty(),
        "a refused request leaves no line"
    );

    let run = w.model.confirm().expect("the shown plan runs");
    assert_eq!(run.kind, PlanKind::Fix(StepId::BASE_TOOLS));
    assert!(w.model.sheet().is_none());
    assert!(w.model.open_sheet(Sheet::Add(AddForm::new())));
}

#[test]
fn the_password_is_cleared_whenever_the_sheet_changes() {
    let mut w = off_world(vec![mac("m1")]);
    let type_in = |w: &mut super::model_tests::World, text: &str| {
        w.model
            .password_field()
            .expect("an admin step takes a password")
            .push_str(text);
    };

    w.model.request_set_up(&StepId::STAYS_AWAKE, "m1");
    type_in(&mut w, "hunter2");
    assert_eq!(w.model.password_len(), 7);
    w.model.cancel_sheet();
    assert_eq!(w.model.password_len(), 0);

    w.model.request_set_up(&StepId::STAYS_AWAKE, "m1");
    assert_eq!(w.model.password_len(), 0, "a new sheet starts empty");
    type_in(&mut w, "hunter2");
    let run = w.model.confirm().expect("a run");
    assert_eq!(run.kind, PlanKind::Fix(StepId::STAYS_AWAKE));
    assert_eq!(w.model.password_len(), 0);

    w.model.request_set_up(&StepId::BASE_TOOLS, "m1");
    assert!(w.model.password_field().is_none());
    w.model.cancel_sheet();

    w.model.request_set_up(&StepId::STAYS_AWAKE, "m1");
    type_in(&mut w, "pw");
    w.model.cancel_sheet();
    assert!(w.model.open_sheet(Sheet::Add(AddForm::new())));
    assert_eq!(w.model.password_len(), 0);
    w.model.close_sheet();
    assert!(w.model.sheet().is_none());
}

#[test]
fn a_set_up_all_ask_opens_with_an_empty_password_and_waits_for_the_sheet_on_show() {
    let mut w = off_world(vec![mac("m1")]);
    w.model.request_set_up(&StepId::STAYS_AWAKE, "m1");
    w.model
        .password_field()
        .unwrap()
        .push_str("typed-for-the-first");
    let asks = w.model.asks.clone();
    let step = notchling_engine::machines::engine_plan::ApprovedStep {
        id: StepId::STAYS_AWAKE,
        title: "Stays awake".into(),
        display: "sudo pmset -a sleep 0".into(),
        waiting: false,
        needs_admin: true,
    };
    let asked = std::thread::spawn(move || asks.ask("m1", &step));
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!w.model.tick(), "the ask waits while another sheet is open");
    assert_eq!(
        fix_sheet(&w.model).plan.kind,
        PlanKind::Fix(StepId::STAYS_AWAKE)
    );
    w.model.cancel_sheet();
    let mut shown = false;
    for _ in 0..200 {
        if w.model.tick() {
            shown = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(shown && fix_sheet(&w.model).asked);
    assert_eq!(w.model.password_len(), 0);
    w.model.cancel_sheet();
    assert!(matches!(
        asked.join().unwrap(),
        notchling_engine::machines::engine_plan::AdminAnswer::None
    ));
}
