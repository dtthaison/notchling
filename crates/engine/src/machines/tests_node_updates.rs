use super::dist_manifest::{DistFile, DistManifest};
use super::engine_plan::{AdminAnswer, AdminNeed, PlanKind};
use super::node_updates::{offer, runnable, MemberBuild, UpdateRow, UpdateState};
use super::registry::Machine;
use super::tests_engine_world::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

const OURS: &str = "abc1234";
const OTHER: &str = "def5678";

fn member(name: &str, os: &str, build: Option<&str>) -> MemberBuild {
    MemberBuild {
        node: format!("node-{name}"),
        name: name.into(),
        os: os.into(),
        arch: if os == "macos" {
            std::env::consts::ARCH.into()
        } else {
            "x86_64".into()
        },
        build: build.map(str::to_string),
        running_shell_jobs: 0,
        moving: false,
    }
}

fn manifest(build: &str, files: &[&str]) -> DistManifest {
    DistManifest {
        build: build.into(),
        files: files
            .iter()
            .map(|f| {
                (
                    f.to_string(),
                    DistFile {
                        sha256: "0".repeat(64),
                        size: 1,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
    }
}

fn machines() -> Vec<Machine> {
    vec![mac("m1"), linux("vps")]
}

fn none() -> BTreeSet<String> {
    BTreeSet::new()
}

fn state(rows: &[UpdateRow], name: &str) -> Option<UpdateState> {
    rows.iter()
        .find(|r| r.machine == name)
        .map(|r| r.state.clone())
}

#[test]
fn same_build_offers_nothing() {
    let members = [
        member("m1", "macos", Some(OURS)),
        member("vps", "linux", Some(OURS)),
    ];
    let rows = offer(Some(OURS), &members, &machines(), None, &none(), false);
    assert!(rows.is_empty(), "{rows:?}");
}

#[test]
fn other_build_is_ready() {
    let members = [
        member("m1", "macos", Some(OTHER)),
        member("vps", "linux", Some(OTHER)),
    ];
    let listed = manifest(OURS, &["notchling-linux-x86_64"]);
    let rows = offer(
        Some(OURS),
        &members,
        &machines(),
        Some(&listed),
        &none(),
        false,
    );
    assert_eq!(state(&rows, "m1"), Some(UpdateState::Ready));
    assert_eq!(state(&rows, "vps"), Some(UpdateState::Ready));
}

#[test]
fn linux_needs_manifest_build() {
    let members = [member("vps", "linux", Some(OTHER))];
    for dist in [
        None,
        Some(manifest(OTHER, &["notchling-linux-x86_64"])),
        Some(manifest(OURS, &[])),
    ] {
        let rows = offer(
            Some(OURS),
            &members,
            &machines(),
            dist.as_ref(),
            &none(),
            false,
        );
        match state(&rows, "vps") {
            Some(UpdateState::NeedsBuild(why)) => {
                assert!(why.contains("tools/build-node.sh"), "{why}")
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn running_shell_jobs_skip_the_machine() {
    let mut busy = member("m1", "macos", Some(OTHER));
    busy.running_shell_jobs = 2;
    let rows = offer(Some(OURS), &[busy], &machines(), None, &none(), false);
    match state(&rows, "m1") {
        Some(UpdateState::Busy(why)) => assert!(why.contains('2'), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(runnable(&rows, &[]).is_empty());
}

#[test]
fn moving_primary_skips() {
    let mut moving = member("m1", "macos", Some(OTHER));
    moving.moving = true;
    let rows = offer(Some(OURS), &[moving], &machines(), None, &none(), false);
    assert!(matches!(state(&rows, "m1"), Some(UpdateState::Busy(_))));
    assert!(runnable(&rows, &[]).is_empty());
}

#[test]
fn member_not_in_machines_says_so() {
    let members = [member("stranger", "linux", Some(OTHER))];
    let rows = offer(Some(OURS), &members, &machines(), None, &none(), false);
    assert_eq!(state(&rows, "stranger"), Some(UpdateState::NotInMachines));
    assert!(runnable(&rows, &[]).is_empty());
    let same = [member("stranger", "linux", Some(OURS))];
    assert!(offer(Some(OURS), &same, &machines(), None, &none(), false).is_empty());
    let unknown = [member("stranger", "macos", None)];
    assert!(offer(Some(OURS), &unknown, &machines(), None, &none(), false).is_empty());
    assert!(offer(None, &members, &machines(), None, &none(), false).is_empty());
}

#[test]
fn dev_build_falls_back_to_binary_step() {
    let members = [
        member("m1", "macos", None),
        member("vps", "linux", Some(OTHER)),
    ];
    let rows = offer(None, &members, &machines(), None, &none(), false);
    assert!(rows.is_empty(), "{rows:?}");
    let failed: BTreeSet<String> = ["vps".to_string()].into();
    let rows = offer(None, &members, &machines(), None, &failed, false);
    assert_eq!(state(&rows, "vps"), Some(UpdateState::Ready));
    assert_eq!(state(&rows, "m1"), None);
    let rows = offer(
        Some(OURS),
        &[member("m1", "macos", None)],
        &machines(),
        None,
        &none(),
        false,
    );
    assert!(rows.is_empty(), "{rows:?}");
    let failed: BTreeSet<String> = ["m1".to_string()].into();
    let rows = offer(
        Some(OURS),
        &[member("m1", "macos", None)],
        &machines(),
        None,
        &failed,
        false,
    );
    assert_eq!(state(&rows, "m1"), Some(UpdateState::Ready));
}

fn world(steps_ok: &[(&str, bool)], seen: &Arc<Mutex<Vec<String>>>) -> World {
    let ok: BTreeMap<String, bool> = steps_ok.iter().map(|(n, b)| (n.to_string(), *b)).collect();
    let seen = seen.clone();
    let mut setup = Setup::new(move |m, _| {
        let name = m.name.clone();
        let good = ok.get(&name).copied().unwrap_or(true);
        let admin = m.os == Some(super::registry::MachineOs::MacOs);
        let seen = seen.clone();
        vec![StepBuilder::new("node-binary")
            .fix(super::step::StepFix::Run(super::step::FixPlan::new(
                format!("update {name}"),
                admin,
                move |_| {
                    lock(&seen).push(name.clone());
                    if good {
                        super::step::FixOutcome::ok(format!("{name} updated"))
                    } else {
                        super::step::FixOutcome::fail(format!("{name} failed"))
                    }
                },
            )))
            .build()]
    });
    setup.machines = vec![mac("m1"), linux("v1"), linux("v2")];
    setup.build()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn update_plan_shows_each_machines_binary_fix_and_asks_a_mac_once() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let w = world(&[], &seen);
    let plan = w
        .engine
        .plan("", PlanKind::UpdateNotchling(names(&["m1", "v1", "v2"])))
        .unwrap();
    assert_eq!(plan.admin, AdminNeed::None);
    let shown: Vec<&str> = plan.steps.iter().map(|s| s.display.as_str()).collect();
    assert_eq!(shown, ["update m1", "update v1", "update v2"]);
    let admin: Vec<bool> = plan.steps.iter().map(|s| s.needs_admin).collect();
    assert_eq!(admin, [true, false, false]);
    assert!(lock(&seen).is_empty(), "a plan runs nothing");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let a = asked.clone();
    let out = w.engine.run_with(
        w.engine.confirm(plan.id, AdminAnswer::None).unwrap(),
        &move |s| {
            lock(&a).push(s.title.clone());
            pw("hunter2-Pw!")
        },
    );
    assert!(out.ok, "{}", out.message);
    assert_eq!(lock(&asked).clone(), ["Update Notchling on m1"]);
    assert_eq!(lock(&seen).clone(), ["m1", "v1", "v2"]);
    assert!(
        w.engine.confirm(plan.id, AdminAnswer::None).is_err(),
        "one confirmation per plan"
    );
}

#[test]
fn update_plan_stops_at_first_failure() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let w = world(&[("v1", false)], &seen);
    let plan = w
        .engine
        .plan("", PlanKind::UpdateNotchling(names(&["v2", "v1", "m1"])))
        .unwrap();
    let out = w
        .engine
        .run(w.engine.confirm(plan.id, AdminAnswer::None).unwrap());
    assert!(!out.ok);
    assert!(out.message.starts_with("v1: v1 failed"), "{}", out.message);
    assert_eq!(lock(&seen).clone(), ["v2", "v1"]);
}

fn checked_world(broken: (&'static str, &'static str), seen: &Arc<Mutex<Vec<String>>>) -> World {
    let seen = seen.clone();
    let mut setup = Setup::new(move |m, _| {
        let name = m.name.clone();
        let seen = seen.clone();
        let check = move |id: &'static str| {
            StepBuilder::new(id).parse(move |_, ctx| {
                if (ctx.machine.name.as_str(), id) == broken {
                    super::step::CheckResult::fail("the daemon exited right away")
                } else {
                    super::step::CheckResult::pass("fine")
                }
            })
        };
        vec![
            check("node-binary")
                .fix(super::step::StepFix::Run(super::step::FixPlan::new(
                    format!("update {name}"),
                    false,
                    move |_| {
                        lock(&seen).push(name.clone());
                        super::step::FixOutcome::ok("restarted")
                    },
                )))
                .build(),
            check("node-service").needs(&["node-binary"]).build(),
        ]
    });
    setup.machines = vec![linux("v1"), linux("v2"), linux("v3")];
    setup.build()
}

fn update_with(broken: (&'static str, &'static str)) -> (super::step::FixOutcome, Vec<String>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let w = checked_world(broken, &seen);
    let plan = w
        .engine
        .plan("", PlanKind::UpdateNotchling(names(&["v2", "v1", "v3"])))
        .unwrap();
    let out = w
        .engine
        .run(w.engine.confirm(plan.id, AdminAnswer::None).unwrap());
    let ran = lock(&seen).clone();
    (out, ran)
}

#[test]
fn update_stops_when_the_service_fails_after_a_machines_update() {
    let (out, ran) = update_with(("v1", "node-service"));
    assert!(!out.ok);
    assert!(out.message.starts_with("v1: "), "{}", out.message);
    assert!(out.message.contains("NODE-SERVICE"), "{}", out.message);
    assert!(
        out.message.contains("the daemon exited right away"),
        "{}",
        out.message
    );
    assert!(
        out.message.ends_with("1 machine(s) updated before it"),
        "{}",
        out.message
    );
    assert_eq!(ran, ["v2", "v1"]);
}

#[test]
fn update_stops_when_the_binary_check_fails_after_a_machines_update() {
    let (out, ran) = update_with(("v2", "node-binary"));
    assert!(!out.ok);
    assert!(out.message.starts_with("v2: "), "{}", out.message);
    assert!(out.message.contains("NODE-BINARY"), "{}", out.message);
    assert!(
        out.message.ends_with("0 machine(s) updated before it"),
        "{}",
        out.message
    );
    assert_eq!(ran, ["v2"]);
}

#[test]
fn update_goes_on_when_every_check_passes_after_each_update() {
    let (out, ran) = update_with(("nobody", "node-service"));
    assert!(out.ok, "{}", out.message);
    assert_eq!(ran, ["v2", "v1", "v3"]);
}

#[test]
fn update_plan_refuses_when_a_display_changed() {
    let display = Arc::new(Mutex::new("update v1".to_string()));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (d, c) = (display.clone(), seen.clone());
    let mut setup = Setup::new(move |_, _| {
        let c = c.clone();
        let shown = lock(&d).clone();
        vec![StepBuilder::new("node-binary")
            .fix(super::step::StepFix::Run(super::step::FixPlan::new(
                shown,
                false,
                move |_| {
                    lock(&c).push("ran".to_string());
                    super::step::FixOutcome::ok("ran")
                },
            )))
            .build()]
    });
    setup.machines = vec![linux("v1")];
    let w = setup.build();
    let plan = w
        .engine
        .plan("", PlanKind::UpdateNotchling(names(&["v1"])))
        .unwrap();
    *lock(&display) = "update v1 somewhere else".into();
    let out = w
        .engine
        .run(w.engine.confirm(plan.id, AdminAnswer::None).unwrap());
    assert!(!out.ok);
    assert!(out.message.contains("needs a look"), "{}", out.message);
    assert!(lock(&seen).is_empty());
}

#[test]
fn update_plan_refuses_odd_lists() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let w = world(&[], &seen);
    for list in [vec![], names(&["v1", "v1"]), names(&["nobody"])] {
        assert!(w.engine.plan("", PlanKind::UpdateNotchling(list)).is_err());
    }
}

#[test]
fn a_cancelled_password_question_stops_before_the_mac() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let w = world(&[], &seen);
    let plan = w
        .engine
        .plan("", PlanKind::UpdateNotchling(names(&["v1", "m1", "v2"])))
        .unwrap();
    let out = w
        .engine
        .run(w.engine.confirm(plan.id, AdminAnswer::None).unwrap());
    assert!(!out.ok);
    assert!(out.message.contains("m1"), "{}", out.message);
    assert_eq!(lock(&seen).clone(), ["v1"]);
}

#[test]
fn update_anyway_runs_a_busy_machine() {
    let mut busy = member("vps", "linux", Some(OTHER));
    busy.running_shell_jobs = 1;
    let members = [member("m1", "macos", Some(OTHER)), busy];
    let listed = manifest(OURS, &["notchling-linux-x86_64"]);
    let rows = offer(
        Some(OURS),
        &members,
        &machines(),
        Some(&listed),
        &none(),
        false,
    );
    assert_eq!(runnable(&rows, &[]), ["m1"]);
    assert_eq!(runnable(&rows, &["vps".to_string()]), ["m1", "vps"]);
    assert_eq!(runnable(&rows, &["m1".to_string()]), ["m1"]);
}
