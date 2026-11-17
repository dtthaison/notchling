use notchling_engine::accounts::Secret;
use notchling_engine::machines::registry::{AdminAccess, Machine, MachineAbilities, MachineOs};
use notchling_engine::machines::step::StepId;

use super::planned;
use crate::cluster::add::{AddFlow, End, Phase};
use crate::cluster::add_rig_tests::{lock, sd_machine, Rig};

fn linux_probe(groups: &'static str) -> impl Fn(&str) -> (String, i32) + Send + Sync + 'static {
    move |probe: &str| {
        let said = if probe.contains("uname -m") {
            "os=Linux\narch=x86_64\nuid=1001\nuser=agent\nhome=/home/agent\nversion=Ubuntu 24.04\ndisk_kb=90000000\ndocker=none"
        } else if probe.contains("git --version") {
            return (String::new(), 1);
        } else if probe.contains("id -nG") {
            return (format!("1001\nos=Linux\ngroups={groups}\nsudo_l=1\n"), 0);
        } else if probe.contains("status --json --peers=false") {
            r#"{"BackendState":"Running","Self":{"TailscaleIPs":["100.64.0.5"]},"CurrentTailnet":{"Name":"t.ts.net"}}"#
        } else {
            return (String::new(), 1);
        };
        (said.to_string(), 0)
    }
}

fn linux_sd(name: &str) -> Machine {
    let mut m = sd_machine(name, "agent");
    m.os = Some(MachineOs::Linux);
    m.set_admin(AdminAccess::RootSsh);
    m.abilities = Some(MachineAbilities::default());
    m
}

#[test]
fn a_linux_sd_machine_with_git_missing_plans_without_a_git_row() {
    let rig = Rig::real(vec![linux_sd("vps-sing-1")]);
    rig.answer_batches(linux_probe("agent"));
    let mut flow = AddFlow::for_machine(rig.parts(), "vps-sing-1").expect("registered");
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan, "{:?}", flow.phase());
    let plan = flow.plan().expect("a plan");
    let ids: Vec<StepId> = plan.rows.iter().map(|r| r.id.clone()).collect();
    assert!(!ids.contains(&StepId::NODE_GIT), "{ids:?}");
    assert!(!ids.contains(&StepId::SERVICE), "{ids:?}");
}

#[test]
fn a_linux_account_that_can_sudo_blocks_install_and_join() {
    let rig = Rig::real(vec![linux_sd("vps-sing-1")]);
    rig.answer_batches(linux_probe("agent sudo"));
    let mut flow = AddFlow::for_machine(rig.parts(), "vps-sing-1").expect("registered");
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan, "{:?}", flow.phase());
    let plan = flow.plan().expect("a plan");
    let row = plan
        .rows
        .iter()
        .find(|r| r.id == StepId::WORKER_USER)
        .expect("Worker user is shown");
    assert!(row.howto, "{row:?}");
    assert!(row.reason.contains("sudo"), "{row:?}");
    assert!(plan.blocked.is_some(), "{plan:?}");
    let why = flow.run(Some(Secret::new("pw"))).expect_err("blocked");
    assert_eq!(Some(why), plan.blocked);
}

#[test]
fn a_linux_account_that_cannot_sudo_is_not_blocked() {
    let rig = Rig::real(vec![linux_sd("vps-sing-1")]);
    rig.answer_batches(linux_probe("agent"));
    let mut flow = AddFlow::for_machine(rig.parts(), "vps-sing-1").expect("registered");
    rig.check(&mut flow);
    let plan = flow.plan().expect("a plan");
    assert!(plan.after.iter().all(|r| r.id != StepId::WORKER_USER));
    assert!(plan.rows.iter().all(|r| r.id != StepId::WORKER_USER));
    assert!(rig
        .engine
        .results("vps-sing-1")
        .get(&StepId::WORKER_USER)
        .is_some_and(|r| r.state.is_pass()));
    assert!(plan.blocked.is_none(), "{:?}", plan.blocked);
}

fn unknown_os(name: &str) -> Machine {
    let mut m = Machine::new(name, &format!("{name}.test"), "agent");
    m.os = None;
    m
}

#[test]
fn a_known_machine_found_to_be_linux_gets_root_ssh_admin() {
    let rig = Rig::new(vec![unknown_os("vps-2")]);
    *lock(&rig.world.os) = MachineOs::Linux;
    *lock(&rig.world.arch) = "x86_64".into();
    let mut flow = AddFlow::for_machine(rig.parts(), "vps-2").expect("registered");
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan, "{:?}", flow.phase());
    let saved = rig.saved("vps-2").unwrap();
    assert_eq!(saved.os, Some(MachineOs::Linux));
    assert_eq!(saved.admin(), AdminAccess::RootSsh);
    let plan = flow.plan().unwrap();
    assert!(plan.blocked.is_none(), "{:?}", plan.blocked);
    assert!(!plan.needs_password);
}

#[test]
fn a_known_machines_chosen_admin_is_never_changed() {
    let mut chosen = Machine::new("vps-3", "vps-3.test", "agent");
    chosen.os = Some(MachineOs::Linux);
    chosen.set_admin(AdminAccess::Sudo);
    let rig = Rig::new(vec![chosen]);
    *lock(&rig.world.os) = MachineOs::Linux;
    let mut flow = AddFlow::for_machine(rig.parts(), "vps-3").expect("registered");
    rig.check(&mut flow);
    assert_eq!(rig.saved("vps-3").unwrap().admin(), AdminAccess::Sudo);
}

#[test]
fn a_howto_the_join_needs_that_waits_at_plan_time_is_rechecked_before_the_join() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.reach) = false;
    let mut flow = planned(&rig, "sondoan@m1");
    let plan = flow.plan().unwrap();
    assert!(
        plan.rows.iter().all(|r| r.id != StepId::NODE_REACH),
        "Reach is waiting on the service, so it isn't listed: {:?}",
        plan.rows
    );
    flow.run(Some(Secret::new("pw"))).expect("runs");
    let end = rig.run_job(&mut flow);
    assert_eq!(
        rig.world.fixed(),
        ["binary", "service"],
        "the join never ran"
    );
    let End::Stopped(stop) = end else {
        panic!("the join went ahead with the Primary out of reach")
    };
    assert!(stop.check_again, "{stop:?}");
    assert!(stop.reason.contains("Reaches the Primary"), "{stop:?}");
    assert_eq!(stop.howto.as_deref(), Some("open the ACL"), "{stop:?}");
    assert_eq!(
        rig.world.fixed(),
        ["binary", "service"],
        "the join never ran"
    );
}

#[test]
fn a_waiting_howto_that_passes_after_the_fixes_lets_the_join_run() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "sondoan@m1");
    flow.run(Some(Secret::new("pw"))).expect("runs");
    assert_eq!(rig.run_job(&mut flow), End::Joined);
    assert_eq!(rig.world.fixed(), ["binary", "service", "joined"]);
}
