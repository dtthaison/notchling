use notchling_engine::machines::engine_plan::PlanKind;
use notchling_engine::machines::step::{CheckResult, StepId};

use super::model_tests::{fix_sheet, lock, mac, scripted_world};

#[test]
fn join_cluster_turns_the_node_on_and_asks_for_a_check() {
    let w = scripted_world(vec![mac("m1"), mac("m2")]);
    let mut model = w.model;
    assert!(model.join_cluster("m1"), "a card without Cluster Node");
    assert!(lock(&w.registry.list)[0].node.is_some());
    assert!(lock(&w.registry.list)[1].node.is_none());
    assert!(
        !model.join_cluster("m1"),
        "a card that already has Cluster Node has nothing to turn on"
    );
    assert!(!model.join_cluster("nobody"));
}

#[test]
fn join_cluster_plans_set_up_for_what_is_missing() {
    let w = scripted_world(vec![mac("m1")]);
    let mut model = w.model;
    assert!(model.join_cluster("m1"));
    model.request_join_plan("m1");
    assert!(
        model.sheet().is_none(),
        "nothing is checked yet, so there is no plan"
    );

    model.engine().show(
        "m1",
        StepId::NODE_BINARY,
        CheckResult::fail("not installed"),
    );
    model.request_join_plan("m1");
    let shown = fix_sheet(&model);
    assert_eq!(shown.plan.kind, PlanKind::SetUpAll);
    assert!(shown.plan.steps.iter().any(|s| s.id == StepId::NODE_BINARY));
}

#[test]
fn the_card_says_where_to_join() {
    let w = scripted_world(vec![mac("m1")]);
    let mut model = w.model;
    let m1 = model.machine("m1").unwrap();
    assert_eq!(model.join_line(&m1), None, "no Cluster Node, no line");

    model.join_cluster("m1");
    let m1 = model.machine("m1").unwrap();
    assert_eq!(
        model.join_line(&m1),
        Some("Join it from the Machines sheet in Notchling"),
    );

    model.engine().show(
        "m1",
        StepId::NODE_JOINED,
        CheckResult::pass("in the cluster"),
    );
    assert_eq!(model.join_line(&m1), None, "a member needs no line");
}
