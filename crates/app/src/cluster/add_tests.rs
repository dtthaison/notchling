use notchling_engine::accounts::Secret;
use notchling_engine::machines::registry::{MachineOs, NodeChoices};
use notchling_engine::machines::step::StepId;

use super::add::{AddFlow, Phase};
use super::add_parts::Account;
use super::add_plan::master_default;
use super::add_rig_tests::{lock, status, Rig, T0};
use super::model::ClusterModel;

fn stopped(flow: &AddFlow) -> super::add::Stop {
    match flow.phase() {
        Phase::Stopped(stop) => stop.clone(),
        other => panic!("not stopped: {other:?}"),
    }
}

fn planned(rig: &Rig, typed: &str) -> AddFlow {
    let mut flow = rig.flow(typed);
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan, "{:?}", flow.phase());
    flow
}

fn titles(flow: &AddFlow) -> Vec<String> {
    flow.plan()
        .expect("a plan")
        .rows
        .into_iter()
        .map(|r| r.title)
        .collect()
}

fn primary_here() -> ClusterModel {
    ClusterModel::from_status(
        &status(Some(("sondoan", "/Users/sondoan")), &[]),
        "a1b2c3d4e5f60718",
    )
}

#[test]
fn address_errors_show_before_check() {
    let rig = Rig::new(vec![]);
    for typed in [
        "root@m5",
        "agent@host:22",
        "a b@host",
        "",
        "ssh://agent@host",
    ] {
        let got = AddFlow::from_address(rig.parts(), typed, &[]);
        assert!(got.is_err(), "{typed}");
        assert!(!got.err().unwrap_or_default().is_empty());
    }
    let taken = ["vps-sing-1".to_string()];
    assert!(AddFlow::from_address(rig.parts(), "agent@vps-sing-1", &taken).is_err());
    let flow = rig.flow("agent@vps-sing-1.tail1234.ts.net");
    assert_eq!(flow.name(), "vps-sing-1");
    assert_eq!(flow.user(), "agent");
    assert_eq!(flow.host(), "vps-sing-1.tail1234.ts.net");
    assert_eq!(flow.phase(), &Phase::Address);
    let bare = rig.flow("m1");
    assert_eq!(bare.user(), "sondoan", "a bare host takes this Mac's user");
    assert!(rig.runner.calls().is_empty(), "nothing ran");
}

#[test]
fn name_can_be_edited_before_check() {
    let rig = Rig::new(vec![]);
    let mut flow = rig.flow("agent@vps-sing-1");
    assert!(flow.set_name("Bad Name", &[]).is_err());
    assert!(flow.set_name("vps", &["vps".to_string()]).is_err());
    flow.set_name("vps", &[]).expect("a good name");
    assert_eq!(flow.name(), "vps");
    rig.check(&mut flow);
    assert!(flow.set_name("other", &[]).is_err(), "not after a check");
    assert_eq!(flow.name(), "vps");
}

#[test]
fn check_runs_no_fix() {
    let rig = Rig::new(vec![]);
    let mut flow = rig.flow("agent@vps-sing-1");
    let requests = flow.check();
    assert_eq!(requests.len(), 1);
    assert_eq!(flow.phase(), &Phase::Checking);
    assert!(
        rig.saved("vps-sing-1").is_none(),
        "nothing saved before the answer"
    );
    flow.reply(requests[0].run(&rig.engine));
    assert_eq!(flow.phase(), &Phase::Plan);
    assert_eq!(rig.fix_calls(), 0, "no Fix call on the runner");
    assert!(rig.world.fixed().is_empty());
    let saved = rig
        .saved("vps-sing-1")
        .expect("saved after Reachable passed");
    assert!(saved.node.is_some());
    assert_eq!(saved.os, Some(MachineOs::MacOs));
    assert_eq!(saved.user, "agent");
    let again = flow.check();
    assert_eq!(again.len(), 1, "Check Again plans again");
    assert!(flow.check().is_empty(), "a check already runs");
    assert_eq!(rig.fix_calls(), 0);
}

#[test]
fn unreachable_stops_before_plan() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.unreachable) = Some("agent@x: Permission denied (publickey).".into());
    let mut flow = rig.flow("agent@vps-sing-1");
    rig.check(&mut flow);
    let stop = stopped(&flow);
    assert!(stop.reason.contains("Permission denied"), "{stop:?}");
    assert!(stop.check_again);
    assert!(stop
        .howto
        .as_deref()
        .is_some_and(|h| h.contains("ssh-copy-id agent@vps-sing-1")));
    assert!(flow.plan().is_none());
    assert!(
        rig.saved("vps-sing-1").is_none(),
        "an unreachable machine is never saved"
    );
    assert_eq!(*lock(&rig.registry.saves), 0);
    assert!(flow.run(None).is_err());
    *lock(&rig.world.unreachable) = None;
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan, "Check Again works after a stop");
}

#[test]
fn host_key_failure_shows_howto() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.unreachable) = Some("Host key verification failed.".into());
    let mut flow = rig.flow("agent@vps-sing-1");
    rig.check(&mut flow);
    let stop = stopped(&flow);
    let howto = stop.howto.expect("a how-to");
    assert!(howto.contains("ssh agent@vps-sing-1"), "{howto}");
    assert!(!howto.contains("ssh-copy-id"), "{howto}");
    assert!(rig.saved("vps-sing-1").is_none());
    *lock(&rig.world.unreachable) = Some("ssh: Could not resolve hostname vps".into());
    rig.check(&mut flow);
    let howto = stopped(&flow).howto.expect("a how-to");
    assert!(
        !howto.contains("ssh-copy-id") && !howto.is_empty(),
        "{howto}"
    );
}

#[test]
fn unsupported_system_stops_before_plan() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.version) = "12.7".into();
    let mut flow = rig.flow("agent@vps-sing-1");
    rig.check(&mut flow);
    let stop = stopped(&flow);
    assert!(
        stop.reason.contains("12.7") && !stop.check_again,
        "{stop:?}"
    );
    assert!(rig.saved("vps-sing-1").is_none());
    *lock(&rig.world.version) = "14.5".into();
    *lock(&rig.world.arch) = "riscv64".into();
    rig.check(&mut flow);
    assert!(stopped(&flow).reason.contains("riscv64"));
}

#[test]
fn another_cluster_stops_before_plan() {
    let rig = Rig::new(vec![]);
    rig.world.set_done(&["binary", "service"]);
    *lock(&rig.world.rival) = true;
    let mut flow = rig.flow("agent@vps-sing-1");
    rig.check(&mut flow);
    let stop = stopped(&flow);
    assert!(
        stop.reason.contains("in another cluster, rival"),
        "{stop:?}"
    );
    assert!(!stop.check_again);
}

#[test]
fn plan_lists_only_failing_steps() {
    let rig = Rig::new(vec![]);
    rig.world.set_done(&["binary"]);
    let flow = planned(&rig, "agent@vps-sing-1");
    let plan = flow.plan().expect("a plan");
    let ids: Vec<StepId> = plan.rows.iter().map(|r| r.id.clone()).collect();
    assert_eq!(ids, [StepId::NODE_SERVICE, StepId::NODE_JOINED]);
    assert_eq!(titles(&flow), ["Notchling service", "Joined the cluster"]);
    assert!(plan.rows[0].admin && !plan.rows[1].admin);
    assert!(plan
        .rows
        .iter()
        .all(|r| !r.why.is_empty() && !r.display.is_empty()));
    assert_eq!(plan.rows[0].reason, "service isn't there");
    assert!(
        plan.needs_password,
        "a Mac's service step needs the admin password"
    );
    assert!(plan.blocked.is_none());
}

#[test]
fn plan_puts_the_join_last_and_leaves_out_how_tos_after_it() {
    let rig = Rig::new(vec![]);
    let flow = planned(&rig, "agent@vps-sing-1");
    let ids: Vec<StepId> = flow
        .plan()
        .unwrap()
        .rows
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(
        ids,
        [
            StepId::NODE_BINARY,
            StepId::NODE_SERVICE,
            StepId::NODE_JOINED
        ]
    );
}

#[test]
fn tailscale_howto_blocks_run() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.tailscale) = false;
    let mut flow = planned(&rig, "agent@vps-sing-1");
    let plan = flow.plan().expect("a plan");
    let tailscale = plan
        .rows
        .iter()
        .find(|r| r.id == StepId::NODE_TAILSCALE)
        .expect("the how-to row");
    assert!(tailscale.howto && tailscale.display.contains("Tailscale"));
    let why = plan.blocked.expect("blocked");
    assert!(why.contains("Tailscale"), "{why}");
    assert!(flow.run(Some(Secret::new("pw"))).is_err());
    assert_eq!(flow.phase(), &Phase::Plan, "a blocked run changes nothing");
    assert!(flow.take_job().is_none());
    assert!(rig.world.fixed().is_empty());
    *lock(&rig.world.tailscale) = true;
    rig.check(&mut flow);
    assert!(flow.plan().expect("a plan").blocked.is_none());
}

#[test]
fn can_be_master_default_needs_filevault_and_same_home() {
    let same = Account {
        user: "sondoan".into(),
        home: "/Users/sondoan".into(),
    };
    let mac = Some(MachineOs::MacOs);
    let on = master_default(
        mac,
        Some(true),
        "sondoan",
        Some("/Users/sondoan"),
        Some(&same),
    );
    assert!(on.on && on.editable && on.reason.is_none());
    let cases = [
        (
            Some(false),
            "sondoan",
            Some("/Users/sondoan"),
            Some(&same),
            "FileVault is off",
        ),
        (
            None,
            "sondoan",
            Some("/Users/sondoan"),
            Some(&same),
            "Can't tell whether FileVault",
        ),
        (
            Some(true),
            "agent",
            Some("/Users/sondoan"),
            Some(&same),
            "same user and home",
        ),
        (
            Some(true),
            "sondoan",
            Some("/Users/agent"),
            Some(&same),
            "same user and home",
        ),
        (
            Some(true),
            "sondoan",
            None,
            Some(&same),
            "same user and home",
        ),
        (
            Some(true),
            "sondoan",
            Some("/Users/sondoan"),
            None,
            "Can't tell the primary's account",
        ),
    ];
    for (fv, user, home, primary, reason) in cases {
        let got = master_default(mac, fv, user, home, primary);
        assert!(!got.on && !got.editable, "{reason}");
        assert!(
            got.reason.as_deref().is_some_and(|r| r.contains(reason)),
            "{got:?}"
        );
    }
}

#[test]
fn can_be_master_default_reads_the_primary_from_cluster_status() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "sondoan@m1");
    flow.seen(&primary_here(), T0);
    let master = flow.plan().unwrap().can_be_master;
    assert!(master.on && master.editable, "{master:?}");

    let other = ClusterModel::from_status(
        &status(Some(("agent", "/home/agent")), &[]),
        "a1b2c3d4e5f60718",
    );
    flow.seen(&other, T0);
    let master = flow.plan().unwrap().can_be_master;
    assert!(
        !master.on
            && master
                .reason
                .as_deref()
                .is_some_and(|r| r.contains("same user"))
    );

    flow.seen(
        &ClusterModel::from_status(&status(None, &[]), "ffffffffffffffff"),
        T0,
    );
    let master = flow.plan().unwrap().can_be_master;
    assert!(
        master
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("primary's account")),
        "{master:?}"
    );

    flow.seen(
        &ClusterModel::from_status(&status(None, &[]), "a1b2c3d4e5f60718"),
        T0,
    );
    assert!(
        flow.plan().unwrap().can_be_master.on,
        "this Mac is the primary: its own account is the answer"
    );

    *lock(&rig.world.file_vault) = Some(false);
    rig.check(&mut flow);
    let master = flow.plan().unwrap().can_be_master;
    assert!(
        !master.on && master.reason.as_deref() == Some("FileVault is off"),
        "{master:?}"
    );
}

#[test]
fn linux_can_be_master_off() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.os) = MachineOs::Linux;
    *lock(&rig.world.arch) = "aarch64".into();
    let mut flow = planned(&rig, "agent@vps-sing-1");
    flow.seen(&primary_here(), T0);
    let master = flow.plan().unwrap().can_be_master;
    assert!(!master.on && !master.editable);
    assert_eq!(
        master.reason.as_deref(),
        Some("Needs an encrypted disk; most VPSes can't")
    );
    flow.set_can_be_master(true);
    assert!(
        !flow.plan().unwrap().can_be_master.on,
        "an off-by-reason choice can't be turned on"
    );
    assert!(
        flow.plan().unwrap().root_confirm,
        "Linux shows the root-over-ssh line"
    );
}

#[test]
fn acting_default_on() {
    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "sondoan@m1");
    flow.seen(&primary_here(), T0);
    let acting = flow.plan().unwrap().acting;
    assert!(acting.on && acting.editable);
    assert!(flow.plan().unwrap().can_be_master.on);
    flow.run(Some(Secret::new("pw"))).expect("runs");
    let saved = rig.saved("m1").expect("saved");
    assert_eq!(
        saved.node.expect("node").choices,
        NodeChoices {
            can_be_master: true,
            acting: true
        },
        "the join's invite reads these from the machine"
    );

    let rig = Rig::new(vec![]);
    let mut flow = planned(&rig, "sondoan@m1");
    flow.set_acting(false);
    assert!(!flow.plan().unwrap().acting.on);
    flow.run(Some(Secret::new("pw"))).expect("runs");
    let choices = rig.saved("m1").unwrap().node.unwrap().choices;
    assert!(!choices.acting && !choices.can_be_master);
}

fn wait(mut ready: impl FnMut() -> bool) {
    for _ in 0..600 {
        if ready() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("never ready");
}

#[path = "add_run_tests.rs"]
mod run;

#[path = "add_review_tests.rs"]
mod review;

#[path = "add_review3_tests.rs"]
mod review3;

#[path = "add_review4_tests.rs"]
mod review4;

#[path = "add_sign_in_tests.rs"]
mod sign_in;

#[path = "add_vault_tests.rs"]
mod vault;
