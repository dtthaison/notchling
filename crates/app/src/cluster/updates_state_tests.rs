use std::collections::BTreeSet;
use std::sync::Arc;

use notchling_engine::machines::engine_plan::{AdminAnswer, ApprovedStep};
use notchling_engine::machines::node_updates::{UpdateRow, UpdateState};
use notchling_engine::machines::registry::{Machine, MachineOs, NodeSettings};
use notchling_engine::machines::step::{FixOutcome, StepId};

use super::add_rig_tests::Rig;
use super::add_sheet::AddPassword;
use super::feed::ClusterFeed;
use super::model::ClusterModel;
use super::updates_state::{Offered, UpdateSession};

fn row(machine: &str, state: UpdateState) -> UpdateRow {
    UpdateRow {
        machine: machine.to_owned(),
        state,
    }
}

fn world(names: &[&str]) -> Rig {
    Rig::new(
        names
            .iter()
            .map(|n| {
                let mut m = Machine::new(n, &format!("{n}.test"), "worker");
                m.os = Some(MachineOs::MacOs);
                m.node = Some(NodeSettings::default());
                m
            })
            .collect(),
    )
}

fn step() -> ApprovedStep {
    ApprovedStep {
        id: StepId::NODE_BINARY,
        title: "m1".into(),
        display: "sudo true".into(),
        waiting: false,
        needs_admin: true,
    }
}

fn asking(session: &UpdateSession) -> std::thread::JoinHandle<AdminAnswer> {
    let asks = session.asks();
    let handle = std::thread::spawn(move || asks.ask("m1", &step()));
    for _ in 0..400 {
        if session.asks().waiting() > 0 {
            return handle;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("nobody asked");
}

fn typed(text: &str) -> AddPassword {
    let mut password = AddPassword::default();
    password.field().push_str(text);
    password
}

#[test]
fn not_now_outlives_the_sheet_and_ends_with_the_build() {
    let feed = ClusterFeed::fixed(ClusterModel::default());
    feed.updates().hide(Some("a1b2c3d"));
    let reopened = feed.updates();
    assert!(reopened.hides(Some("a1b2c3d")), "closed and opened again");
    assert!(!reopened.hides(Some("b2c3d4e")), "a new build shows it");
    assert!(!reopened.hides(None));
}

#[test]
fn reopening_mid_run_keeps_the_run_and_refuses_a_second() {
    let feed = ClusterFeed::fixed(ClusterModel::default());
    let session = feed.updates();
    assert!(session.begin());
    feed.updates().dismissed();
    assert!(feed.updates().running(), "Finish can't clear a running run");
    assert!(!feed.updates().begin(), "no second run beside it");
    feed.updates().finish(FixOutcome::ok("done"));
    assert!(!session.running());
    assert_eq!(session.outcome(), Some(FixOutcome::ok("done")));
    assert_eq!(
        feed.updates().outcome(),
        Some(FixOutcome::ok("done")),
        "a result that came while the sheet was shut is still there"
    );
    session.dismissed();
    assert_eq!(session.outcome(), None, "Finish clears it");
    assert!(session.begin());
}

#[test]
fn an_admin_question_survives_the_sheet_and_waits_to_be_answered() {
    let session = Arc::new(UpdateSession::default());
    assert!(session.begin());
    let asked = asking(&session);
    assert_eq!(session.pull(), (true, true));
    assert_eq!(session.pending_target().as_deref(), Some("m1"));
    assert_eq!(session.pull(), (true, false));
    let mut password = typed("pw");
    assert!(session.answer(true, &mut password));
    let AdminAnswer::Password(secret) = asked.join().unwrap() else {
        panic!("the password was not sent")
    };
    assert_eq!(secret.expose(), "pw");
    assert_eq!(session.pending_target(), None);
}

#[test]
fn choosing_a_vault_entry_answers_the_question_without_a_typed_password() {
    let session = UpdateSession::default();
    assert!(session.begin());
    let asked = asking(&session);
    session.pull();
    assert_eq!(session.pending_machine().as_deref(), Some("m1"));
    assert!(session.answer_vault());
    assert!(matches!(asked.join().unwrap(), AdminAnswer::Vault));
    assert!(!session.answer_vault(), "nothing left to answer");
}

#[test]
fn return_in_an_empty_password_field_asks_nothing_and_cancel_still_cancels() {
    let session = UpdateSession::default();
    assert!(session.begin());
    let asked = asking(&session);
    session.pull();
    let mut empty = AddPassword::default();
    assert!(!session.answer(true, &mut empty), "Continue is disabled");
    assert_eq!(
        session.pending_target().as_deref(),
        Some("m1"),
        "still asking"
    );
    assert!(session.answer(false, &mut empty));
    assert!(matches!(asked.join().unwrap(), AdminAnswer::None));
    assert!(!session.answer(true, &mut typed("pw")), "nothing to answer");
}

#[test]
fn the_plan_follows_the_rows_that_can_run() {
    let rig = world(&["m1", "m2"]);
    let anyway = BTreeSet::new();
    let mut offered = Offered::default();
    let mut rows = vec![
        row("m1", UpdateState::Ready),
        row("m2", UpdateState::Busy("1 shell job(s) running".into())),
    ];
    assert!(offered.sync(&rig.engine, &rows, &anyway));
    assert_eq!(offered.shown().unwrap().steps.len(), 1);
    assert_eq!(offered.button(), ("Update 1 Machine".into(), true));
    assert!(
        !offered.sync(&rig.engine, &rows, &anyway),
        "nothing changed"
    );

    rows[1].state = UpdateState::Ready;
    assert!(offered.sync(&rig.engine, &rows, &anyway), "m2 turned Ready");
    assert_eq!(offered.shown().unwrap().steps.len(), 2);
    assert_eq!(offered.button(), ("Update 2 Machines".into(), true));

    rows[0].state = UpdateState::Busy("a move to another primary is running".into());
    assert!(offered.sync(&rig.engine, &rows, &anyway), "m1 turned Busy");
    assert_eq!(offered.shown().unwrap().steps.len(), 1);
    assert_eq!(offered.button(), ("Update 1 Machine".into(), true));

    rows[1].state = UpdateState::Busy("x".into());
    assert!(offered.sync(&rig.engine, &rows, &anyway));
    assert!(offered.shown().is_none());
    assert!(!offered.button().1, "nothing can run");
    let chosen: BTreeSet<String> = ["m1".to_owned()].into();
    assert!(offered.sync(&rig.engine, &rows, &chosen), "Update Anyway");
    assert_eq!(offered.button(), ("Update 1 Machine".into(), true));
}

#[test]
fn a_replaced_plan_is_cancelled_and_reset_forgets_it() {
    let rig = world(&["m1", "m2"]);
    let anyway = BTreeSet::new();
    let mut offered = Offered::default();
    let mut rows = vec![row("m1", UpdateState::Ready)];
    offered.sync(&rig.engine, &rows, &anyway);
    let old = offered.shown().unwrap().id;
    rows.push(row("m2", UpdateState::Ready));
    offered.sync(&rig.engine, &rows, &anyway);
    assert!(
        rig.engine.confirm(old, AdminAnswer::None).is_err(),
        "the old plan was cancelled"
    );
    offered.reset(Some(&rig.engine));
    assert!(offered.shown().is_none());
    assert!(offered.sync(&rig.engine, &rows, &anyway), "planned afresh");
    assert!(offered.take().is_some());
    assert!(offered.shown().is_none());
}
