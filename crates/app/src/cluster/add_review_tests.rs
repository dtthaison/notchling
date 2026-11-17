use notchling_engine::accounts::Secret;
use notchling_engine::machines::registry::{Machine, MachineAbilities, MachineOs};
use notchling_engine::machines::step::StepId;

use super::{planned, stopped};
use crate::cluster::add::{AddFlow, End, Phase};
use crate::cluster::add_rig_tests::{lock, sd_machine, status, Rig};
use crate::cluster::add_runs::AddRuns;
use crate::cluster::feed::ClusterFeed;
use crate::cluster::model::ClusterModel;

fn known(name: &str, user: &str) -> Machine {
    let mut m = sd_machine(name, user);
    m.abilities = Some(MachineAbilities::default());
    m
}

fn ids(flow: &AddFlow) -> Vec<StepId> {
    flow.plan()
        .expect("a plan")
        .rows
        .into_iter()
        .map(|r| r.id)
        .collect()
}

#[test]
fn join_cluster_writes_probed_facts_for_a_known_machine() {
    let rig = Rig::new(vec![known("m1", "sondoan")]);
    let before = rig.saved("m1").unwrap();
    assert!(before.abilities.as_ref().unwrap().home.is_none());
    let mut flow = AddFlow::for_machine(rig.parts(), "m1").expect("a registered machine");
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan, "{:?}", flow.phase());
    let after = rig.saved("m1").unwrap();
    let abilities = after.abilities.clone().expect("abilities");
    assert_eq!(abilities.home.as_deref(), Some("/Users/sondoan"));
    assert_eq!(abilities.arch.as_deref(), Some("arm64"));
    assert_eq!(abilities.os_version.as_deref(), Some("14.5"));
    assert!(after.file_vault, "FileVault was read");
    assert_eq!(
        after.jobs, before.jobs,
        "the sd worker's jobs are untouched"
    );
    assert!(after.node.is_some());
    let service = flow
        .plan()
        .unwrap()
        .rows
        .into_iter()
        .find(|r| r.id == StepId::NODE_SERVICE)
        .expect("the service row");
    assert!(
        service.display.contains("/Users/sondoan"),
        "the display knows the home: {}",
        service.display
    );
    flow.run(Some(Secret::new("pw"))).expect("runs");
    assert_eq!(rig.run_job(&mut flow), End::Joined);
}

#[test]
fn check_again_keeps_what_the_first_check_saved_and_fills_in_new_facts() {
    let rig = Rig::new(vec![]);
    let mut flow = rig.flow("sondoan@m1");
    rig.check(&mut flow);
    assert_eq!(
        rig.saved("m1").unwrap().abilities.unwrap().notchling_app,
        None
    );
    *lock(&rig.world.app) = Some(true);
    *lock(&rig.world.arch) = "x86_64".into();
    rig.check(&mut flow);
    let abilities = rig.saved("m1").unwrap().abilities.unwrap();
    assert_eq!(abilities.notchling_app, Some(true));
    assert_eq!(abilities.arch.as_deref(), Some("x86_64"));
    assert_eq!(abilities.home.as_deref(), Some("/Users/sondoan"));
}

#[test]
fn vps_without_abilities_gets_its_arch() {
    let rig = Rig::new(vec![sd_machine("vps-sing-1", "agent")]);
    assert!(rig.saved("vps-sing-1").unwrap().abilities.is_none());
    let mut flow = AddFlow::for_machine(rig.parts(), "vps-sing-1").expect("registered");
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan, "{:?}", flow.phase());
    let saved = rig.saved("vps-sing-1").unwrap();
    assert_eq!(saved.os, Some(MachineOs::MacOs));
    let abilities = saved.abilities.expect("abilities were written");
    assert_eq!(abilities.arch.as_deref(), Some("arm64"));
    assert_eq!(abilities.home.as_deref(), Some("/Users/agent"));
}

#[test]
fn app_yes_plans_app_closed() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.app) = Some(true);
    *lock(&rig.world.app_open) = true;
    rig.world.set_done(&["binary"]);
    let flow = planned(&rig, "sondoan@m1");
    let saved = rig.saved("m1").unwrap();
    assert_eq!(saved.abilities.unwrap().notchling_app, Some(true));
    let plan = flow.plan().unwrap();
    let closed = plan
        .rows
        .iter()
        .find(|r| r.id == StepId::NODE_APP_CLOSED)
        .expect("App Closed is planned on the first Check");
    assert!(closed.howto);
    assert_eq!(ids(&flow).last(), Some(&StepId::NODE_JOINED));
}

#[test]
fn sd_machine_plan_has_no_sd_steps_and_keeps_git() {
    let rig = Rig::real(vec![sd_machine("vps-sing-1", "agent")]);
    rig.answer_batches(|probe| {
        let said = if probe.contains("uname -m") {
            "os=Darwin\narch=arm64\nuid=501\nuser=agent\nhome=/Users/agent\nversion=14.5\ndisk_kb=90000000\ndocker=none"
        } else if probe.contains("git --version") {
            return ("clt=missing".to_string(), 1);
        } else if probe.contains("status --json --peers=false") {
            r#"{"BackendState":"Running","Self":{"TailscaleIPs":["100.64.0.5"]},"CurrentTailnet":{"Name":"t.ts.net"}}"#
        } else {
            return (String::new(), 1);
        };
        (said.to_string(), 0)
    });
    let mut flow = AddFlow::for_machine(rig.parts(), "vps-sing-1").expect("registered");
    let before = rig.saved("vps-sing-1").unwrap();
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan, "{:?}", flow.phase());
    let plan = flow.plan().expect("a plan");
    let ids: Vec<StepId> = plan.rows.iter().map(|r| r.id.clone()).collect();
    assert!(!ids.contains(&StepId::CLAUDE_LOGIN), "{ids:?}");
    assert!(!ids.contains(&StepId::SERVICE), "{ids:?}");
    assert!(ids.contains(&StepId::NODE_GIT), "Git is kept: {ids:?}");
    assert!(plan.blocked.is_none(), "{:?}", plan.blocked);
    assert_eq!(rig.saved("vps-sing-1").unwrap().jobs, before.jobs);
}

#[test]
fn begin_seeds_the_primary_account() {
    let rig = Rig::new(vec![]);
    let model = ClusterModel::from_status(
        &status(Some(("sondoan", "/Users/sondoan")), &[]),
        "ffffffffffffffff",
    );
    let feed = ClusterFeed::fixed(model);
    let runs = feed.adds();
    assert!(runs.begin(rig.flow("sondoan@m1")));
    super::wait(|| matches!(runs.get("m1").map(|f| f.phase().clone()), Some(Phase::Plan)));
    let master = runs.get("m1").unwrap().plan.unwrap().can_be_master;
    assert!(master.on && master.editable, "{master:?}");

    let other = AddRuns::default();
    assert!(other.begin(rig.flow("sondoan@m2")));
    super::wait(|| {
        matches!(
            other.get("m2").map(|f| f.phase().clone()),
            Some(Phase::Plan)
        )
    });
    let master = other.get("m2").unwrap().plan.unwrap().can_be_master;
    assert!(!master.on, "no model seen: the account can't be told");
}

#[test]
fn a_howto_the_join_does_not_need_shows_but_does_not_stop_the_run() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.git) = false;
    let mut flow = planned(&rig, "sondoan@m1");
    let plan = flow.plan().unwrap();
    let git = plan
        .rows
        .iter()
        .find(|r| r.id == StepId::NODE_GIT)
        .expect("the how-to is shown");
    assert!(git.howto);
    assert!(plan.blocked.is_none());
    flow.run(Some(Secret::new("pw"))).expect("runs");
    assert!(matches!(
        flow.phase(),
        Phase::Running { step: 1, of: 3, .. }
    ));
    assert_eq!(rig.run_job(&mut flow), End::Joined);
    assert_eq!(rig.world.fixed(), ["binary", "service", "joined"]);
}

#[test]
fn a_howto_the_join_needs_still_stops_the_run() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.app) = Some(true);
    *lock(&rig.world.app_open) = true;
    rig.world.set_done(&["binary"]);
    let mut flow = planned(&rig, "sondoan@m1");
    flow.run(Some(Secret::new("pw"))).expect("runs");
    let End::Stopped(stop) = rig.run_job(&mut flow) else {
        panic!("the join went ahead with the app open")
    };
    assert!(stop.check_again, "{stop:?}");
    assert!(stop.reason.contains("Notchling app closed"), "{stop:?}");
    assert_eq!(stop.howto.as_deref(), Some("quit the app"));
    assert_eq!(rig.world.fixed(), ["service"], "the join never ran");
    assert_eq!(stopped(&flow), stop);
    *lock(&rig.world.app_open) = false;
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan, "Check Again re-plans");
}

#[test]
fn failing_howtos_after_the_join_are_advice() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.codex) = false;
    let mut flow = planned(&rig, "sondoan@m1");
    let plan = flow.plan().unwrap();
    assert!(
        !plan.rows.iter().any(|r| r.id == StepId::NODE_CODEX_LOGIN),
        "not a step of the run"
    );
    let advice: Vec<StepId> = plan.after.iter().map(|r| r.id.clone()).collect();
    assert_eq!(advice, [StepId::NODE_CODEX_LOGIN]);
    assert!(plan.after[0].howto && !plan.after[0].display.is_empty());
    assert!(plan.blocked.is_none());
    assert_eq!(ids(&flow).last(), Some(&StepId::NODE_JOINED));
    flow.run(Some(Secret::new("pw"))).expect("runs");
    assert!(matches!(
        flow.phase(),
        Phase::Running { step: 1, of: 3, .. }
    ));
    assert_eq!(rig.run_job(&mut flow), End::Joined);
    assert_eq!(rig.world.fixed(), ["binary", "service", "joined"]);
}

#[test]
fn a_recheck_that_cant_tell_stops_the_run() {
    let rig = Rig::new(vec![]);
    lock(&rig.world.unsure).insert("binary".into());
    let mut flow = planned(&rig, "sondoan@m1");
    flow.run(Some(Secret::new("pw"))).expect("runs");
    let End::Stopped(stop) = rig.run_job(&mut flow) else {
        panic!("went on past a step that can't be told")
    };
    assert!(stop.check_again);
    assert!(stop.reason.contains("Notchling installed"), "{stop:?}");
    assert!(stop.reason.contains("can't tell about binary"), "{stop:?}");
    assert_eq!(rig.world.fixed(), ["binary"]);
}
