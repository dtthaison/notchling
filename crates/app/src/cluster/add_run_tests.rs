use notchling_engine::accounts::Secret;
use notchling_engine::machines::registry::MachineOs;
use notchling_engine::machines::runner::OFF;
use notchling_engine::machines::step::{CheckResult, StepId};

use super::{planned, stopped, wait};
use crate::cluster::add::{AddFlow, End, Phase, JOIN_WAIT_SECS};
use crate::cluster::add_parts::AddParts;
use crate::cluster::add_rig_tests::{lock, sd_machine, status, Rig, NODE, T0};
use crate::cluster::add_runs::AddRuns;
use crate::cluster::model::ClusterModel;
use crate::machines::data::Setup;

#[test]
fn run_needs_the_password_the_plan_asks_for() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "agent@vps-sing-1");
    assert!(flow.run(None).is_err());
    assert!(flow.run(Some(Secret::new(""))).is_err());
    assert_eq!(flow.phase(), &Phase::Plan);
    assert!(flow.run(Some(Secret::new("pw"))).is_ok());
    assert!(matches!(flow.phase(), Phase::Running { .. }));
    assert!(
        flow.run(Some(Secret::new("pw"))).is_err(),
        "a plan confirms once"
    );
}

#[test]
fn run_refuses_changed_display() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "agent@vps-sing-1");
    {
        let mut list = lock(&rig.registry.list);
        let m = list.iter_mut().find(|m| m.name == "vps-sing-1").unwrap();
        m.abilities.as_mut().unwrap().home = Some("/Users/elsewhere".into());
    }
    let why = flow.run(Some(Secret::new("pw"))).expect_err("changed");
    assert!(why.contains("changed"), "{why}");
    assert!(flow.take_job().is_none());
    assert!(rig.world.fixed().is_empty(), "nothing ran");
    let stop = stopped(&flow);
    assert!(stop.check_again);
}

#[test]
fn run_stops_at_first_failure_with_check_again() {
    let rig = Rig::new(vec![]);
    lock(&rig.world.fails).insert("service".into());
    let mut flow = planned(&rig, "agent@vps-sing-1");
    flow.run(Some(Secret::new("pw"))).expect("runs");
    let end = rig.run_job(&mut flow);
    assert_eq!(
        rig.world.fixed(),
        ["binary", "service"],
        "the join never ran"
    );
    let End::Stopped(stop) = end else {
        panic!("not stopped")
    };
    assert!(stop.reason.contains("Notchling service"), "{stop:?}");
    assert!(stop.reason.contains("could not service"), "{stop:?}");
    assert!(stop.check_again);
    assert_eq!(stopped(&flow), stop);
    lock(&rig.world.fails).clear();
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan);
    assert_eq!(
        flow.plan()
            .unwrap()
            .rows
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        [StepId::NODE_SERVICE, StepId::NODE_JOINED],
        "the finished binary step is no longer listed"
    );
}

#[test]
fn a_run_ends_joined_and_waits_for_the_member() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "agent@vps-sing-1");
    flow.run(Some(Secret::new("pw"))).expect("runs");
    assert!(matches!(
        flow.phase(),
        Phase::Running { step: 1, of: 3, .. }
    ));
    let end = rig.run_job(&mut flow);
    assert_eq!(end, End::Joined);
    assert_eq!(
        rig.world.fixed(),
        ["binary", "service", "joined"],
        "the join runs last"
    );
    assert_eq!(flow.phase(), &Phase::Joining { since: T0 });
}

#[test]
fn done_when_member_active_and_online() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "agent@vps-sing-1");
    flow.run(Some(Secret::new("pw"))).expect("runs");
    rig.run_job(&mut flow);
    let me = "a1b2c3d4e5f60718";
    let pending =
        ClusterModel::from_status(&status(None, &[(NODE, "elsewhere", "pending", false)]), me);
    flow.seen(&pending, T0 + 5.0);
    assert!(
        matches!(flow.phase(), Phase::Joining { .. }),
        "pending isn't done"
    );
    let offline =
        ClusterModel::from_status(&status(None, &[(NODE, "elsewhere", "active", false)]), me);
    flow.seen(&offline, T0 + 6.0);
    assert!(
        matches!(flow.phase(), Phase::Joining { .. }),
        "offline isn't done"
    );
    let online =
        ClusterModel::from_status(&status(None, &[(NODE, "elsewhere", "active", true)]), me);
    flow.seen(&online, T0 + 7.0);
    assert_eq!(
        flow.phase(),
        &Phase::Done,
        "matched by node id, not by name"
    );
}

#[test]
fn done_matches_by_name_when_the_node_id_is_unknown() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "agent@vps-sing-1");
    flow.run(Some(Secret::new("pw"))).expect("runs");
    rig.run_job(&mut flow);
    let me = "a1b2c3d4e5f60718";
    let by_name = ClusterModel::from_status(
        &status(None, &[("ffeeddccbbaa9988", "vps-sing-1", "active", true)]),
        me,
    );
    flow.seen(&by_name, T0 + 1.0);
    assert_eq!(flow.phase(), &Phase::Done);
}

#[test]
fn a_member_that_never_shows_up_times_out_with_check_again() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "agent@vps-sing-1");
    flow.run(Some(Secret::new("pw"))).expect("runs");
    rig.run_job(&mut flow);
    let none = ClusterModel::from_status(&status(None, &[]), "a1b2c3d4e5f60718");
    flow.seen(&none, T0 + JOIN_WAIT_SECS - 1.0);
    assert!(matches!(flow.phase(), Phase::Joining { .. }));
    flow.seen(&none, T0 + JOIN_WAIT_SECS + 1.0);
    let stop = stopped(&flow);
    assert!(
        stop.check_again && stop.reason.contains("vps-sing-1"),
        "{stop:?}"
    );
}

#[test]
fn an_old_reply_is_ignored() {
    let rig = Rig::new(vec![]);
    let mut flow = rig.flow("agent@vps-sing-1");
    let first = flow.check();
    let stale = first[0].run(&rig.engine);
    flow.reply(first[0].run(&rig.engine));
    assert_eq!(flow.phase(), &Phase::Plan);
    let again = flow.check();
    flow.reply(stale);
    assert_eq!(
        flow.phase(),
        &Phase::Checking,
        "a reply from an earlier round changes nothing"
    );
    flow.reply(again[0].run(&rig.engine));
    assert_eq!(flow.phase(), &Phase::Plan);
}

#[test]
fn run_outlives_the_sheet() {
    let rig = Rig::new(vec![]);
    let runs = AddRuns::default();
    let flow = rig.flow("agent@vps-sing-1");
    assert!(runs.begin(flow));
    wait(|| {
        matches!(
            runs.get("vps-sing-1").map(|f| f.phase().clone()),
            Some(Phase::Plan)
        )
    });
    runs.run("vps-sing-1", Some(Secret::new("pw")))
        .expect("runs");
    wait(|| {
        matches!(
            runs.get("vps-sing-1").map(|f| f.phase().clone()),
            Some(Phase::Joining { .. })
        )
    });
    assert_eq!(rig.world.fixed(), ["binary", "service", "joined"]);
    assert!(
        runs.get("vps-sing-1").is_some(),
        "the feed's runs still hold it, whatever the view did"
    );
    let online = ClusterModel::from_status(
        &status(None, &[(NODE, "vps-sing-1", "active", true)]),
        "a1b2c3d4e5f60718",
    );
    runs.seen(&online);
    assert_eq!(runs.get("vps-sing-1").unwrap().phase(), &Phase::Done);
    assert!(runs.lines().is_empty());
    assert!(runs.running().is_none());
}

#[test]
fn running_line_names_the_step() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "agent@vps-sing-1");
    assert!(flow.running_line().is_none());
    flow.run(Some(Secret::new("pw"))).expect("runs");
    assert_eq!(flow.running_line(), Some(("vps-sing-1".to_string(), 1, 3)));
}

#[test]
fn join_cluster_turns_node_on_and_checks_node_steps_only() {
    let rig = Rig::new(vec![
        sd_machine("m1", "sondoan"),
        sd_machine("m2", "sondoan"),
    ]);
    rig.engine.show(
        "m1",
        StepId::SERVICE,
        CheckResult::fail("sd service isn't loaded"),
    );
    assert!(rig.saved("m1").unwrap().node.is_none());
    let mut flow = AddFlow::for_machine(rig.parts(), "m1").expect("a registered machine");
    let m1 = rig.saved("m1").unwrap();
    assert!(m1.node.is_some(), "Join Cluster turns Cluster Node on");
    assert!(rig.saved("m2").unwrap().node.is_none(), "only that machine");
    assert_eq!(flow.phase(), &Phase::Address);
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan, "{:?}", flow.phase());
    assert_eq!(rig.fix_calls(), 0);
    let ids: Vec<StepId> = flow
        .plan()
        .unwrap()
        .rows
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert!(!ids.contains(&StepId::SERVICE), "{ids:?}");
    assert!(ids.contains(&StepId::NODE_JOINED));
    let kept = rig.engine.results("m1");
    assert_eq!(
        kept.get(&StepId::SERVICE)
            .map(|r| r.state.text().to_owned()),
        Some("sd service isn't loaded".to_owned()),
        "the sd steps' results are kept"
    );
    flow.run(Some(Secret::new("pw"))).expect("runs");
    rig.run_job(&mut flow);
    assert!(
        !rig.world.fixed().contains(&"sd-service".to_owned()),
        "{:?}",
        rig.world.fixed()
    );
    let after = rig.saved("m1").unwrap();
    assert_eq!(
        after.jobs, m1.jobs,
        "the sd worker's settings are untouched"
    );
    assert_eq!(after.user, "sondoan");
    assert!(after.node.unwrap().choices.acting);
    assert!(AddFlow::for_machine(rig.parts(), "nobody").is_err());
}

#[test]
fn fixture_root_runs_nothing() {
    let dir = std::env::temp_dir().join(format!("notchling-add-fixture-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let setup = Setup::for_run(Some(&dir), true, false).expect("a fixture setup");
    assert!(setup.fixture && !setup.live);
    let parts = AddParts::live(&setup);
    assert_eq!(parts.off, Some(OFF));
    let existing = parts.names();
    let mut flow = AddFlow::from_address(parts.clone(), "agent@vps-sing-1", &existing).expect("ok");
    for probe in flow.check() {
        let reply = probe.run(&parts.engine);
        flow.reply(reply);
    }
    let stop = stopped(&flow);
    assert!(stop.reason.contains(OFF), "{stop:?}");
    assert!(parts.registry.all().is_empty(), "nothing saved");
    assert!(flow.run(None).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn password_never_in_debug_or_rows() {
    let pw = "hunter2-secret";
    let rig = Rig::new(vec![]);
    lock(&rig.world.fails).insert("service".into());
    let mut flow = planned(&rig, "agent@vps-sing-1");
    let plan = flow.plan().expect("a plan");
    flow.run(Some(Secret::new(pw))).expect("runs");
    let before = format!("{flow:?} {plan:?}");
    assert!(!before.contains(pw));
    let end = rig.run_job(&mut flow);
    let End::Stopped(stop) = &end else {
        panic!("not stopped")
    };
    assert!(
        stop.reason.contains("could not service"),
        "the fix said it: {stop:?}"
    );
    for text in [
        format!("{flow:?}"),
        format!("{:?}", flow.plan()),
        format!("{end:?}"),
        format!("{:?}", flow.phase()),
        flow.log().join("\n"),
        format!("{:?}", rig.runner.calls()),
    ] {
        assert!(!text.contains(pw), "{text}");
    }
}

#[test]
fn the_real_catalog_makes_a_plan_for_a_linux_machine_with_probes_only() {
    let rig = Rig::real(vec![]);
    rig.answer_batches(|probe| {
        let said = if probe.contains("uname -m") {
            "os=Linux\narch=x86_64\nhome=/home/agent\nversion=Ubuntu 24.04\ndisk_kb=90000000\ndocker=none"
        } else if probe.contains("sudo_l") {
            "1001\nos=Linux\ngroups=agent\nsudo_l=1"
        } else if probe.contains("tailscale status") {
            r#"{"BackendState":"Running","Self":{"TailscaleIPs":["100.64.0.5"]},"CurrentTailnet":{"Name":"t.ts.net"}}"#
        } else if probe.contains("git --version") {
            "git version 2.43.0"
        } else {
            return (String::new(), 1);
        };
        (said.to_string(), 0)
    });
    let mut flow = rig.flow("agent@vps-sing-1");
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan, "{:?}", flow.phase());
    let plan = flow.plan().expect("a plan");
    let ids: Vec<StepId> = plan.rows.iter().map(|r| r.id.clone()).collect();
    assert!(ids.contains(&StepId::NODE_BINARY), "{ids:?}");
    assert!(
        !ids.contains(&StepId::NODE_GIT) && !ids.contains(&StepId::NODE_TAILSCALE),
        "{ids:?}"
    );
    assert_eq!(ids.last(), Some(&StepId::NODE_JOINED), "{ids:?}");
    assert!(plan.root_confirm && !plan.needs_password);
    assert!(plan.can_be_master.reason.is_some());
    let saved = rig.saved("vps-sing-1").expect("saved");
    assert_eq!(saved.os, Some(MachineOs::Linux));
    assert!(saved.node.is_some());
    assert!(
        rig.runner.calls().iter().all(|c| c
            .argv
            .last()
            .is_some_and(|s| s.contains("out=$( /bin/sh -c "))),
        "only read-only probes ran"
    );
}

#[test]
fn the_feed_holds_the_runs_and_a_change_bumps_its_generation() {
    let rig = Rig::new(vec![]);
    let feed = crate::cluster::feed::ClusterFeed::none();
    let before = feed.generation();
    let runs = feed.adds();
    assert!(runs.begin(rig.flow("agent@vps-sing-1")));
    wait(|| {
        matches!(
            runs.get("vps-sing-1").map(|f| f.phase().clone()),
            Some(Phase::Plan)
        )
    });
    assert!(
        feed.generation() > before,
        "the view repaints when a run moves"
    );
}
