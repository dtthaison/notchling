use std::collections::BTreeSet;

use notchling_engine::machines::engine_plan::AdminAnswer;
use notchling_engine::machines::node_updates::{UpdateRow, UpdateState};
use notchling_engine::machines::registry::{Machine, MachineOs, NodeSettings};
use notchling_engine::machines::step::FixOutcome;

use super::add_rig_tests::Rig;
use super::feed::ClusterFeed;
use super::model::ClusterModel;
use super::updates_state::Offered;

fn sync(
    offered: &mut Offered,
    engine: &notchling_engine::machines::engine::SetupEngine,
    rows: &[UpdateRow],
    anyway: &BTreeSet<String>,
) -> bool {
    let names = notchling_engine::machines::node_updates::runnable(
        rows,
        &anyway.iter().cloned().collect::<Vec<_>>(),
    );
    offered.sync_names(engine, names)
}

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
fn the_plan_follows_the_rows_that_can_run() {
    let rig = world(&["m1", "m2"]);
    let anyway = BTreeSet::new();
    let mut offered = Offered::default();
    let mut rows = vec![
        row("m1", UpdateState::Ready),
        row("m2", UpdateState::Busy("1 shell job(s) running".into())),
    ];
    assert!(sync(&mut offered, &rig.engine, &rows, &anyway));
    assert_eq!(offered.shown().unwrap().steps.len(), 1);
    assert_eq!(offered.button(), ("Update m1".into(), true));
    assert!(
        !sync(&mut offered, &rig.engine, &rows, &anyway),
        "nothing changed"
    );

    rows[1].state = UpdateState::Ready;
    assert!(
        sync(&mut offered, &rig.engine, &rows, &anyway),
        "m2 turned Ready"
    );
    assert_eq!(offered.shown().unwrap().steps.len(), 2);
    assert_eq!(offered.button(), ("Update 2 Machines".into(), true));

    rows[0].state = UpdateState::Busy("a move to another primary is running".into());
    assert!(
        sync(&mut offered, &rig.engine, &rows, &anyway),
        "m1 turned Busy"
    );
    assert_eq!(offered.shown().unwrap().steps.len(), 1);
    assert_eq!(offered.button(), ("Update m2".into(), true));

    rows[1].state = UpdateState::Busy("x".into());
    assert!(sync(&mut offered, &rig.engine, &rows, &anyway));
    assert!(offered.shown().is_none());
    assert!(!offered.button().1, "nothing can run");
    let chosen: BTreeSet<String> = ["m1".to_owned()].into();
    assert!(
        sync(&mut offered, &rig.engine, &rows, &chosen),
        "Update Anyway"
    );
    assert_eq!(offered.button(), ("Update m1".into(), true));
}

#[test]
fn a_replaced_plan_is_cancelled_and_reset_forgets_it() {
    let rig = world(&["m1", "m2"]);
    let anyway = BTreeSet::new();
    let mut offered = Offered::default();
    let mut rows = vec![row("m1", UpdateState::Ready)];
    sync(&mut offered, &rig.engine, &rows, &anyway);
    let old = offered.shown().unwrap().id;
    rows.push(row("m2", UpdateState::Ready));
    sync(&mut offered, &rig.engine, &rows, &anyway);
    assert!(
        rig.engine.confirm(old, AdminAnswer::None).is_err(),
        "the old plan was cancelled"
    );
    offered.reset(Some(&rig.engine));
    assert!(offered.shown().is_none());
    assert!(
        sync(&mut offered, &rig.engine, &rows, &anyway),
        "planned afresh"
    );
    assert!(offered.take().is_some());
    assert!(offered.shown().is_none());
}

#[test]
fn nothing_selected_gives_a_reason_and_no_plan() {
    let rig = world(&["m1"]);
    let mut offered = Offered::default();
    assert!(offered.sync_names(&rig.engine, Vec::new()));
    assert!(offered.shown().is_none());
    assert_eq!(offered.error(), Some("No machine can be updated now."));
    assert!(offered.names().is_empty());
    assert!(offered.sync_names(&rig.engine, vec!["m1".to_owned()]));
    assert_eq!(offered.error(), None);
    assert_eq!(offered.names(), ["m1"]);
    assert!(!offered.sync_names(&rig.engine, vec!["m1".to_owned()]));
}

#[test]
fn a_machine_the_registry_lacks_gets_the_engines_reason() {
    let rig = world(&["m1"]);
    let mut offered = Offered::default();
    offered.sync_names(&rig.engine, vec!["ghost".to_owned()]);
    assert!(offered.shown().is_none());
    assert!(offered.error().is_some_and(|e| e.contains("ghost")));
}
