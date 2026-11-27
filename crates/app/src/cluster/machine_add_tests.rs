use notchling_engine::accounts::Secret;
use notchling_engine::machines::registry::{AdminAccess, Machine, MachineOs};
use notchling_engine::machines::step::{CheckResult, StepId};

use super::add::{AddFlow, Phase as FlowPhase};
use super::add_rig_tests::{lock, sd_machine, status, Rig, NODE, T0};
use super::add_runs::AddRuns;
use super::add_sheet::Draft;
use super::add_source::AddSource;
use super::machine_add::{remove, MachineAdd, Phase};
use super::model::ClusterModel;
use super::not_joined::{annotated, checked_line, not_joined, NotJoined, Reach, RouteLine};

fn draft(address: &str, name: &str) -> Draft {
    Draft {
        address: address.into(),
        name: name.into(),
        ..Draft::default()
    }
}

fn model(members: &[(&str, &str, &str, bool)]) -> ClusterModel {
    ClusterModel::from_status(&status(None, members), "a1b2c3d4e5f60718")
}

fn added(rig: &Rig, typed: &str, org: Option<&str>) -> MachineAdd {
    let mut add = MachineAdd::new();
    let probe = add
        .begin(&rig.parts(), &draft(typed, ""), org.map(str::to_owned))
        .expect("a good address");
    assert_eq!(add.phase(), &Phase::Checking);
    let answer = probe.run(&rig.engine);
    add.answered(&rig.parts(), answer);
    add
}

fn nothing(_: &str) -> Option<CheckResult> {
    None
}

fn listed(rig: &Rig, m: &ClusterModel) -> Vec<NotJoined> {
    not_joined(&rig.parts().registry.all(), &nothing, m)
}

#[test]
fn add_saves_the_machine_and_installs_and_joins_nothing() {
    let rig = Rig::new(vec![]);
    let add = added(&rig, "deploy@app-1.example", None);
    assert!(matches!(add.phase(), Phase::Saved { name, .. } if name == "app-1"));
    let saved = rig.saved("app-1").expect("saved");
    assert_eq!(saved.host, "app-1.example");
    assert_eq!(saved.user, "deploy");
    assert_eq!(saved.os, Some(MachineOs::MacOs));
    assert!(saved.node.is_none(), "not a node until Join Cluster");
    assert!(saved.jobs.is_empty());
    assert_eq!(saved.organization, None);
    assert_eq!(rig.fix_calls(), 0);
    assert!(rig.world.fixed().is_empty());
    assert!(rig.engine.running("app-1").is_none());
    assert_eq!(*lock(&rig.registry.saves), 1);
    let kept = rig.engine.results("app-1");
    assert!(kept
        .get(&StepId::REACHABLE)
        .is_some_and(|r| r.state.is_pass()));
    assert!(!kept.contains_key(&StepId::NODE_JOINED));
}

#[test]
fn a_saved_machine_is_listed_under_not_in_the_cluster() {
    let rig = Rig::new(vec![]);
    added(&rig, "deploy@app-1.example", None);
    added(&rig, "ci@build-2.example", None);
    let names: Vec<String> = listed(&rig, &model(&[]))
        .into_iter()
        .map(|m| m.name)
        .collect();
    assert_eq!(names, ["app-1", "build-2"]);
    let member = model(&[("1122334455667788", "app-1", "active", true)]);
    let names: Vec<String> = listed(&rig, &member).into_iter().map(|m| m.name).collect();
    assert_eq!(names, ["build-2"], "a member is in the other list");
}

#[test]
fn join_cluster_on_an_added_machine_still_runs_the_node_plan() {
    let rig = Rig::new(vec![]);
    added(&rig, "deploy@app-1.example", None);
    assert!(rig.saved("app-1").unwrap().node.is_none());
    let mut flow = AddFlow::for_machine(rig.parts(), "app-1").expect("registered");
    assert!(rig.saved("app-1").unwrap().node.is_some());
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &FlowPhase::Plan, "{:?}", flow.phase());
    let ids: Vec<StepId> = flow
        .plan()
        .expect("a plan")
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
    assert_eq!(rig.fix_calls(), 0, "the plan is shown before anything runs");
    flow.run(Some(Secret::new("pw"))).expect("runs");
    rig.run_job(&mut flow);
    let fixed = rig.world.fixed();
    assert!(
        ["binary", "service", "joined"]
            .iter()
            .all(|f| fixed.contains(&(*f).to_owned())),
        "{fixed:?}"
    );
}

#[test]
fn an_unreachable_machine_is_not_saved_and_check_again_works() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.unreachable) = Some("deploy@x: Permission denied (publickey).".into());
    let mut add = added(&rig, "deploy@app-1.example", None);
    let Phase::Stopped(stop) = add.phase().clone() else {
        panic!("{:?}", add.phase());
    };
    assert!(stop.reason.contains("Permission denied"), "{stop:?}");
    assert!(stop.check_again);
    assert!(stop
        .howto
        .as_deref()
        .is_some_and(|h| h.contains("ssh-copy-id deploy@app-1.example")));
    assert!(rig.saved("app-1").is_none());
    assert_eq!(*lock(&rig.registry.saves), 0);
    *lock(&rig.world.unreachable) = None;
    let probe = add
        .begin(&rig.parts(), &draft("deploy@app-1.example", ""), None)
        .expect("again");
    let answer = probe.run(&rig.engine);
    assert!(add.answered(&rig.parts(), answer));
    assert!(matches!(add.phase(), Phase::Saved { .. }));
    assert!(rig.saved("app-1").is_some());
}

#[test]
fn a_taken_name_or_bad_address_is_refused_before_any_ssh() {
    let rig = Rig::new(vec![sd_machine("m1", "sondoan")]);
    let mut add = MachineAdd::new();
    for typed in ["", "root@x", "agent@host:99999", "a b@host", "host:ab"] {
        assert!(add.begin(&rig.parts(), &draft(typed, ""), None).is_err());
    }
    assert!(add
        .begin(&rig.parts(), &draft("deploy@app-1.example", "m1"), None)
        .is_err());
    assert_eq!(add.phase(), &Phase::Form);
    assert!(rig.runner.calls().is_empty(), "nothing ran");
}

#[test]
fn a_name_taken_while_checking_stops_without_overwriting() {
    let rig = Rig::new(vec![]);
    let mut add = MachineAdd::new();
    let probe = add
        .begin(&rig.parts(), &draft("deploy@app-1.example", ""), None)
        .expect("ok");
    let mut other = Machine::new("app-1", "other.example", "x");
    other.os = Some(MachineOs::Linux);
    lock(&rig.registry.list).push(other);
    let answer = probe.run(&rig.engine);
    assert!(!add.answered(&rig.parts(), answer));
    assert!(matches!(add.phase(), Phase::Stopped(_)));
    assert_eq!(rig.saved("app-1").unwrap().host, "other.example");
}

#[test]
fn an_answer_of_an_older_round_is_dropped() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.unreachable) = Some("no route".into());
    let mut add = MachineAdd::new();
    let first = add
        .begin(&rig.parts(), &draft("deploy@app-1.example", ""), None)
        .expect("ok");
    let (now, late) = (first.run(&rig.engine), first.run(&rig.engine));
    add.answered(&rig.parts(), now);
    assert!(matches!(add.phase(), Phase::Stopped(_)));
    *lock(&rig.world.unreachable) = None;
    add.begin(&rig.parts(), &draft("deploy@app-1.example", ""), None)
        .expect("a second round");
    assert!(!add.answered(&rig.parts(), late));
    assert_eq!(add.phase(), &Phase::Checking, "round 1 is stale");
    assert!(rig.saved("app-1").is_none());
}

#[test]
fn the_organization_is_kept_and_blank_means_personal() {
    let rig = Rig::new(vec![]);
    added(&rig, "deploy@app-1.example", Some("org-acme"));
    assert_eq!(
        rig.saved("app-1").unwrap().organization.as_deref(),
        Some("org-acme")
    );
    added(&rig, "deploy@app-2.example", Some("  "));
    let two = rig.saved("app-2").unwrap();
    assert_eq!(two.organization, None);
    assert_eq!(
        two.organization_id(),
        notchling_engine::organizations::PERSONAL_ID
    );
}

#[test]
fn a_linux_machine_gets_root_ssh_admin_and_a_name_can_be_typed() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.os) = MachineOs::Linux;
    let mut add = MachineAdd::new();
    let probe = add
        .begin(&rig.parts(), &draft("deploy@10.0.0.7", "vps-a"), None)
        .expect("ok");
    let answer = probe.run(&rig.engine);
    add.answered(&rig.parts(), answer);
    let saved = rig.saved("vps-a").expect("saved under the typed name");
    assert_eq!(saved.os, Some(MachineOs::Linux));
    assert_eq!(saved.admin(), AdminAccess::RootSsh);
}

#[test]
fn an_unsupported_machine_is_saved_with_a_note() {
    let rig = Rig::new(vec![]);
    *lock(&rig.world.version) = "12.7".into();
    let add = added(&rig, "deploy@old-mac.example", None);
    let Phase::Saved { note, .. } = add.phase() else {
        panic!("{:?}", add.phase());
    };
    assert!(
        note.as_deref().is_some_and(|n| n.contains("12.7")),
        "{note:?}"
    );
    assert!(rig.saved("old-mac").is_some());
}

#[test]
fn remove_takes_a_not_joined_machine_out_of_machines() {
    let rig = Rig::new(vec![]);
    added(&rig, "deploy@app-1.example", None);
    added(&rig, "ci@build-2.example", None);
    remove(&rig.parts(), &model(&[]), "app-1").expect("removed");
    assert!(rig.saved("app-1").is_none());
    assert!(rig.saved("build-2").is_some());
    assert!(
        rig.engine.results("app-1").is_empty(),
        "its checks are forgotten"
    );
    assert!(remove(&rig.parts(), &model(&[]), "app-1").is_err());
}

#[test]
fn remove_refuses_a_machine_that_is_in_the_cluster() {
    let rig = Rig::new(vec![]);
    added(&rig, "deploy@app-1.example", None);
    let member = model(&[("1122334455667788", "app-1", "active", true)]);
    let why = remove(&rig.parts(), &member, "app-1").expect_err("a member");
    assert!(why.contains("Leave Cluster"), "{why}");
    assert!(rig.saved("app-1").is_some());
    let gone = model(&[("1122334455667788", "app-1", "removed", false)]);
    remove(&rig.parts(), &gone, "app-1").expect("a removed member may go");
}

#[test]
fn check_reads_the_machine_and_stamps_the_time() {
    let rig = Rig::new(vec![]);
    added(&rig, "deploy@app-1.example", None);
    let source = AddSource::over(rig.parts());
    let before = source.not_joined(&model(&[]));
    assert_eq!(before[0].checked_s, None);
    *lock(&rig.world.unreachable) = Some("no route to host".into());
    source.check("app-1");
    let after = source.not_joined(&model(&[]));
    assert_eq!(after[0].reach, Reach::Down);
    assert!(after[0].checked_s.is_some());
    *lock(&rig.world.unreachable) = None;
    source.check("app-1");
    assert_eq!(source.not_joined(&model(&[]))[0].reach, Reach::Up);
}

#[test]
fn the_source_keeps_fixed_stamps_for_renders() {
    let rig = Rig::new(vec![]);
    added(&rig, "deploy@app-1.example", None);
    let source = AddSource::over(rig.parts()).with_checked("app-1", 1_800_000_000);
    assert_eq!(
        source.not_joined(&model(&[]))[0].checked_s,
        Some(1_800_000_000)
    );
}

fn row(reach: Reach, at: Option<i64>) -> NotJoined {
    NotJoined {
        name: "app-1".into(),
        host: "app-1.example".into(),
        user: "deploy".into(),
        os: Some(MachineOs::Linux),
        node_on: false,
        reach,
        checked_s: at,
        route: RouteLine::Hidden,
    }
}

#[test]
fn the_status_line_says_reach_and_age() {
    let now = T0 as i64;
    assert_eq!(
        checked_line(&row(Reach::Unchecked, None), now),
        "Not checked yet"
    );
    assert_eq!(
        checked_line(&row(Reach::Up, Some(now - 5)), now),
        "Reachable · Checked just now"
    );
    assert_eq!(
        checked_line(&row(Reach::Down, Some(now - 600)), now),
        "Not reachable · Checked 10 minutes ago"
    );
    assert_eq!(
        checked_line(&row(Reach::Unsure, Some(now - 7300)), now),
        "Can't tell if it is reachable · Checked 2 hours ago"
    );
    assert_eq!(checked_line(&row(Reach::Up, None), now), "Reachable");
    assert_eq!(
        checked_line(&row(Reach::Up, Some(now + 90)), now),
        "Reachable · Checked just now",
        "a clock that ran backwards never reads as a negative age"
    );
}

#[test]
fn annotation_reads_reach_from_the_results() {
    let list = vec![row(Reach::Unchecked, None)];
    let up = |_: &str| Some(CheckResult::pass("up"));
    let none = |_: &str| None;
    let no_time = |_: &str| None::<i64>;
    assert_eq!(annotated(list.clone(), &up, &no_time)[0].reach, Reach::Up);
    let down = |_: &str| Some(CheckResult::fail("no"));
    assert_eq!(
        annotated(list.clone(), &down, &no_time)[0].reach,
        Reach::Down
    );
    let unsure = |_: &str| Some(CheckResult::cant_tell("?"));
    let got = annotated(list.clone(), &unsure, &|_| Some(7));
    assert_eq!((got[0].reach, got[0].checked_s), (Reach::Unsure, Some(7)));
    assert_eq!(annotated(list, &none, &no_time)[0].reach, Reach::Unchecked);
}

#[test]
fn the_cluster_only_takes_machines_while_one_exists() {
    assert!(model(&[]).accepts_machines());
    assert!(!ClusterModel::default().accepts_machines());
}

#[cfg(unix)]
#[test]
fn no_local_api_op_changes_machines() {
    let wire = include_str!("../../../local-api/src/wire.rs");
    let start = wire.find("pub mod ops {").expect("the ops");
    let end = wire[start..].find("\n}\n").expect("their end") + start;
    let names: Vec<&str> = wire[start..end]
        .lines()
        .filter(|l| l.trim_start().starts_with("pub const"))
        .filter_map(|l| l.split('"').nth(1))
        .collect();
    assert!(names.len() > 50, "the op list was read: {}", names.len());
    let reads = [
        "machines.list",
        "machines.check",
        "machines.plan",
        "machines.peers",
    ];
    let card = ["machines.request", "machines.apply"];
    let bad: Vec<&&str> = names
        .iter()
        .filter(|n| n.starts_with("machine") || n.contains(".machine"))
        .filter(|n| !reads.contains(n) && !card.contains(n))
        .collect();
    assert!(
        bad.is_empty(),
        "only a card the user allowed changes a machine: {bad:?}"
    );
    for op in card {
        assert!(names.contains(&op), "{op}");
    }
}

#[test]
fn remove_refuses_while_the_cluster_is_not_read() {
    let rig = Rig::new(vec![]);
    added(&rig, "deploy@app-1.example", None);
    let why = remove(&rig.parts(), &ClusterModel::default(), "app-1").expect_err("unread");
    assert!(why.contains("not read"), "{why}");
    assert!(rig.saved("app-1").is_some());
}

#[test]
fn remove_refuses_a_member_known_by_its_node_id_under_another_name() {
    let rig = Rig::new(vec![]);
    added(&rig, "deploy@app-1.example", None);
    let mut flow = AddFlow::for_machine(rig.parts(), "app-1").expect("registered");
    rig.world.set_done(&["binary", "service"]);
    rig.check(&mut flow);
    let renamed = model(&[(NODE, "kitchen-mac", "active", true)]);
    let why = remove(&rig.parts(), &renamed, "app-1").expect_err("a member by id");
    assert!(why.contains("Leave Cluster"), "{why}");
    assert!(rig.saved("app-1").is_some());
    let elsewhere = model(&[("ffffffffffffffff", "kitchen-mac", "active", true)]);
    remove(&rig.parts(), &elsewhere, "app-1").expect("not a member");
}

#[test]
fn remove_refuses_a_machine_the_checks_say_has_joined() {
    let rig = Rig::new(vec![]);
    added(&rig, "deploy@app-1.example", None);
    let mut flow = AddFlow::for_machine(rig.parts(), "app-1").expect("registered");
    rig.world.set_done(&["binary", "service", "joined"]);
    rig.check(&mut flow);
    let why = remove(&rig.parts(), &model(&[]), "app-1").expect_err("joined");
    assert!(why.contains("Leave Cluster"), "{why}");
}

#[test]
fn a_probe_from_a_closed_sheet_never_lands_on_the_next_one() {
    let rig = Rig::new(vec![]);
    let mut first = MachineAdd::new();
    let old = first
        .begin(&rig.parts(), &draft("deploy@app-1.example", ""), None)
        .expect("a good address");
    let mut second = MachineAdd::new();
    let fresh = second
        .begin(&rig.parts(), &draft("ci@build-2.example", ""), None)
        .expect("a good address");
    let late = old.run(&rig.engine);
    assert!(!second.answered(&rig.parts(), late));
    assert_eq!(second.phase(), &Phase::Checking);
    assert!(rig.saved("app-1").is_none());
    assert!(rig.saved("build-2").is_none());
    let own = fresh.run(&rig.engine);
    assert!(second.answered(&rig.parts(), own));
    assert!(rig.saved("build-2").is_some());
}

#[test]
fn a_check_after_a_removal_stamps_nothing_and_forget_drops_the_stamp() {
    let rig = Rig::new(vec![]);
    added(&rig, "deploy@app-1.example", None);
    let source = AddSource::over(rig.parts()).with_checked("app-1", 1_800_000_000);
    remove(&rig.parts(), &model(&[]), "app-1").expect("removed");
    source.forget("app-1");
    source.check("app-1");
    added(&rig, "deploy@app-1.example", None);
    assert_eq!(source.not_joined(&model(&[]))[0].checked_s, None);
}

#[test]
fn a_stopped_join_row_goes_with_the_machine_and_a_running_one_holds_it() {
    let rig = Rig::new(vec![]);
    added(&rig, "deploy@app-1.example", None);
    let runs = AddRuns::default();
    let flow = AddFlow::for_machine(rig.parts(), "app-1").expect("registered");
    assert!(runs.begin(flow));
    assert!(runs.busy("app-1"), "a check in flight holds the machine");
    assert!(!runs.busy("build-2"));
    for _ in 0..500 {
        if !runs.busy("app-1") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!runs.busy("app-1"));
    remove(&rig.parts(), &model(&[]), "app-1").expect("removed");
    runs.dismiss("app-1");
    assert!(runs.get("app-1").is_none());
}

#[test]
fn a_refusal_without_a_check_is_plain_and_has_no_check_again() {
    let rig = Rig::new(vec![]);
    let mut add = MachineAdd::new();
    let probe = add
        .begin(&rig.parts(), &draft("deploy@app-1.example", ""), None)
        .expect("a good address")
        .with_existing(&["app-1"]);
    let answer = probe.run(&rig.engine);
    assert!(!add.answered(&rig.parts(), answer));
    let Phase::Stopped(stop) = add.phase() else {
        panic!("{:?}", add.phase());
    };
    assert!(stop.reason.contains("app-1"), "{stop:?}");
    assert!(stop.howto.is_none(), "{stop:?}");
    assert!(!stop.check_again, "{stop:?}");
    assert!(rig.saved("app-1").is_none());
}
