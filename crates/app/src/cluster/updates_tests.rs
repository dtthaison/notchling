use std::collections::{BTreeMap, BTreeSet};

use notchling_engine::machines::dist_manifest::DistManifest;
use notchling_engine::machines::node_updates::UpdateState;
use notchling_engine::machines::registry::{Machine, MachineOs, NodeSettings};
use serde_json::{json, Value};

use super::add_rig_tests::Rig;
use super::model::ClusterModel;
use super::updates::{
    banner, execute, idle_lines, lines, members, plan, rows_for, run_label, selected, Kind, NotNow,
};

const ME: &str = "a1b2c3d4e5f60718";
const OURS: &str = "a1b2c3d";
const OTHER: &str = "9f8e7d6";
const M1: &str = "1122334455667788";
const VPS: &str = "99aabbccddeeff00";

fn member(id: &str, name: &str, os: &str, arch: &str, build: Option<&str>) -> Value {
    let mut versions = json!({
        "notchling": "0.9.0", "proto": 1, "features": [], "schema": 1,
        "os": os, "arch": arch,
    });
    if let Some(build) = build {
        versions["build"] = json!(build);
    }
    json!({
        "id": id, "name": name, "role": "worker", "canBeMaster": false,
        "online": true, "client": false, "headless": true, "reference": false,
        "state": "active", "versions": versions,
    })
}

fn status(members: Vec<Value>, jobs: Value) -> Value {
    let mut all = vec![member(ME, "m5", "macos", "aarch64", Some(OURS))];
    all[0]["role"] = json!("home");
    all[0]["reference"] = json!(true);
    all.extend(members);
    json!({
        "cluster": {"id": "c1", "name": "home", "term": 1, "home": ME},
        "me": ME,
        "members": all,
        "jobs": jobs,
        "link": true, "tailscale": true, "listening": [47823],
    })
}

fn model(members: Vec<Value>, jobs: Value) -> ClusterModel {
    ClusterModel::from_status(&status(members, jobs), ME)
}

fn node(name: &str, os: MachineOs) -> Machine {
    let mut m = Machine::new(name, &format!("{name}.test"), "worker");
    m.os = Some(os);
    m.node = Some(NodeSettings::default());
    m
}

fn none(_: &str) -> Option<String> {
    None
}

fn rows(
    model: &ClusterModel,
    machines: &[Machine],
) -> Vec<notchling_engine::machines::node_updates::UpdateRow> {
    rows_for(
        Some(OURS),
        model,
        machines,
        &none,
        None,
        &BTreeSet::new(),
        false,
    )
}

fn two_old() -> ClusterModel {
    model(
        vec![
            member(M1, "m1", "macos", "aarch64", Some(OTHER)),
            member(VPS, "vps-sing-1", "linux", "x86_64", Some(OTHER)),
        ],
        json!([]),
    )
}

fn manifest(build: &str) -> DistManifest {
    let sha = "0".repeat(64);
    serde_json::from_value(json!({
        "build": build,
        "files": {"notchling-linux-x86_64": {"sha256": sha, "size": 10}},
    }))
    .unwrap()
}

#[test]
fn banner_names_the_machines() {
    let machines = [
        node("m1", MachineOs::MacOs),
        node("vps-sing-1", MachineOs::Linux),
    ];
    let shown = banner(&rows(&two_old(), &machines)).expect("two machines differ");
    assert_eq!(
        shown.text,
        "2 machines run another Notchling build: m1, vps-sing-1"
    );
    assert_eq!(shown.button, "Update 2 Machines…");
    assert_eq!(shown.count, 2);

    let one = model(
        vec![member(M1, "m1", "macos", "aarch64", Some(OTHER))],
        json!([]),
    );
    let single = banner(&rows(&one, &machines)).unwrap();
    assert_eq!(single.text, "1 machine runs another Notchling build: m1");
    assert_eq!(single.button, "Update 1 Machine…");

    let same = model(
        vec![member(M1, "m1", "macos", "aarch64", Some(OURS))],
        json!([]),
    );
    assert!(banner(&rows(&same, &machines)).is_none());
}

#[test]
fn not_now_hides_until_next_build() {
    let mut hidden = NotNow::default();
    assert!(!hidden.hides(Some(OURS)));
    hidden.hide(Some(OURS));
    assert!(hidden.hides(Some(OURS)));
    assert!(!hidden.hides(Some("b2c3d4e")), "a new build shows it again");
    assert!(!hidden.hides(None));
    let mut dev = NotNow::default();
    dev.hide(None);
    assert!(dev.hides(None));
    assert!(!dev.hides(Some(OURS)));
}

#[test]
fn needs_build_row_says_build_nodes_first() {
    let machines = [node("vps-sing-1", MachineOs::Linux)];
    let one = model(
        vec![member(VPS, "vps-sing-1", "linux", "x86_64", Some(OTHER))],
        json!([]),
    );
    let got = rows(&one, &machines);
    assert!(matches!(got[0].state, UpdateState::NeedsBuild(_)));
    let shown = lines(&got, &BTreeSet::new());
    assert_eq!(shown[0].kind, Kind::Skipped);
    assert!(shown[0].text.contains("tools/build-node.sh"));
    assert!(!shown[0].offers_anyway());
    assert!(selected(&got, &BTreeSet::new(), &BTreeSet::new()).is_empty());

    let listed = rows_for(
        Some(OURS),
        &one,
        &machines,
        &none,
        Some(&manifest(OURS)),
        &BTreeSet::new(),
        false,
    );
    assert_eq!(listed[0].state, UpdateState::Ready);
}

#[test]
fn busy_row_offers_update_anyway() {
    let machines = [node("m1", MachineOs::MacOs)];
    let busy = model(
        vec![member(M1, "m1", "macos", "aarch64", Some(OTHER))],
        json!([{"id": "j1", "node": M1, "tool": "shell", "state": "running"}]),
    );
    let got = rows(&busy, &machines);
    assert!(matches!(got[0].state, UpdateState::Busy(_)));
    let mut anyway = BTreeSet::new();
    let shown = lines(&got, &anyway);
    assert_eq!(shown[0].kind, Kind::Busy);
    assert!(shown[0].offers_anyway());
    assert!(shown[0].text.contains("1 shell job"));
    assert!(selected(&got, &anyway, &BTreeSet::new()).is_empty());

    anyway.insert("m1".to_owned());
    let chosen = lines(&got, &anyway);
    assert_eq!(chosen[0].kind, Kind::Chosen);
    let names = selected(&got, &anyway, &BTreeSet::new());
    assert_eq!(run_label(&names), "Update m1");
}

#[test]
fn shell_jobs_and_moves_come_from_the_status() {
    let machines = [
        node("m1", MachineOs::MacOs),
        node("vps-sing-1", MachineOs::Linux),
    ];
    let jobs = json!([
        {"id": "j1", "node": M1, "tool": "shell", "state": "running"},
        {"id": "j2", "node": M1, "tool": "shell", "state": "running"},
        {"id": "j3", "node": M1, "tool": "shell", "state": "done"},
        {"id": "j4", "node": M1, "tool": "claude", "state": "running"},
        {"id": "j5", "node": VPS, "tool": "shell", "state": "failed"},
    ]);
    let m = model(
        vec![
            member(M1, "m1", "macos", "aarch64", Some(OTHER)),
            member(VPS, "vps-sing-1", "linux", "x86_64", Some(OTHER)),
        ],
        jobs,
    );
    let got = members(&m, &machines, &none);
    assert_eq!(got.len(), 2, "this Mac is never offered itself");
    let m1 = got.iter().find(|g| g.name == "m1").unwrap();
    let vps = got.iter().find(|g| g.name == "vps-sing-1").unwrap();
    assert_eq!(m1.running_shell_jobs, 2, "only running shell jobs count");
    assert_eq!(vps.running_shell_jobs, 0);
    assert!(!m1.moving && !vps.moving);
    assert_eq!(m1.build.as_deref(), Some(OTHER));
    assert_eq!((m1.os.as_str(), m1.arch.as_str()), ("macos", "aarch64"));

    let mut moving = status(
        vec![member(M1, "m1", "macos", "aarch64", Some(OTHER))],
        json!([]),
    );
    moving["move"] = json!({
        "to": M1, "step": "freeze", "moving": [], "staying": [],
        "cut": [], "stopped": [], "failed": [], "reason": null,
    });
    let m = ClusterModel::from_status(&moving, ME);
    let got = members(&m, &machines, &none);
    assert!(got.iter().find(|g| g.name == "m1").unwrap().moving);
}

#[test]
fn a_member_is_matched_to_its_machine_by_node_id_then_name() {
    let machines = [
        node("studio", MachineOs::MacOs),
        node("m1", MachineOs::MacOs),
    ];
    let m = model(
        vec![member(M1, "mac-studio", "macos", "aarch64", Some(OTHER))],
        json!([]),
    );
    let by_id = |name: &str| (name == "studio").then(|| M1.to_owned());
    let got = members(&m, &machines, &by_id);
    assert_eq!(got[0].name, "studio", "the registry name, found by node id");

    let renamed = model(
        vec![member(M1, "m1", "macos", "aarch64", Some(OTHER))],
        json!([]),
    );
    let by_name = members(&renamed, &machines, &none);
    assert_eq!(by_name[0].name, "m1", "no node id known: the name decides");

    let stranger = model(
        vec![member(M1, "stranger", "macos", "aarch64", Some(OTHER))],
        json!([]),
    );
    let got = rows_for(
        Some(OURS),
        &stranger,
        &machines,
        &none,
        None,
        &BTreeSet::new(),
        false,
    );
    assert_eq!(got[0].state, UpdateState::NotInMachines);
    let line = lines(&got, &BTreeSet::new());
    assert_eq!(line[0].kind, Kind::Skipped);
    assert!(line[0].text.contains("stranger"));
}

#[test]
fn removed_and_waiting_members_are_not_offered() {
    let machines = [node("m1", MachineOs::MacOs)];
    let mut gone = member(M1, "m1", "macos", "aarch64", Some(OTHER));
    gone["state"] = json!("removed");
    let mut pending = member(VPS, "vps-sing-1", "linux", "x86_64", Some(OTHER));
    pending["state"] = json!("pending");
    let m = model(vec![gone, pending], json!([]));
    assert!(rows(&m, &machines).is_empty());
}

fn world(names: &[&str]) -> Rig {
    Rig::new(names.iter().map(|n| node(n, MachineOs::MacOs)).collect())
}

fn ready_rows(
    rig: &Rig,
    names: &[&str],
) -> Vec<notchling_engine::machines::node_updates::UpdateRow> {
    let _ = rig;
    names
        .iter()
        .map(|n| notchling_engine::machines::node_updates::UpdateRow {
            machine: (*n).to_owned(),
            state: UpdateState::Ready,
        })
        .collect()
}

#[test]
fn run_goes_one_at_a_time_and_stops_at_failure() {
    let rig = world(&["m1", "m2", "m3"]);
    rig.world.set_done(&["service"]);
    let rows = ready_rows(&rig, &["m1", "m2", "m3"]);
    let shown = plan(&rig.engine, &rows, &BTreeSet::new()).expect("one plan for all three");
    assert_eq!(shown.steps.len(), 3);
    let done = execute(&rig.engine, &shown, &BTreeMap::new());
    assert!(done.ok, "{}", done.message);
    assert_eq!(rig.world.fixed(), ["binary", "binary", "binary"]);

    let rig = world(&["m1", "m2", "m3"]);
    rig.world.fails.lock().unwrap().insert("binary".into());
    let rows = ready_rows(&rig, &["m1", "m2", "m3"]);
    let shown = plan(&rig.engine, &rows, &BTreeSet::new()).unwrap();
    let stopped = execute(&rig.engine, &shown, &BTreeMap::new());
    assert!(!stopped.ok);
    assert!(stopped.message.contains("m1"), "{}", stopped.message);
    assert!(stopped.message.contains("0 machine(s) updated"));
    assert_eq!(
        rig.world.fixed(),
        ["binary"],
        "m2 and m3 were never touched"
    );
}

#[test]
fn a_busy_machine_runs_only_after_update_anyway() {
    let rig = world(&["m1", "m2"]);
    let mut rows = ready_rows(&rig, &["m1", "m2"]);
    rows[1].state = UpdateState::Busy("1 shell job(s) running".into());
    let held = plan(&rig.engine, &rows, &BTreeSet::new()).unwrap();
    assert_eq!(held.steps.len(), 1, "m2 is skipped");
    rig.engine.cancel(held.id);

    let anyway: BTreeSet<String> = ["m2".to_owned()].into();
    let both = plan(&rig.engine, &rows, &anyway).unwrap();
    assert_eq!(both.steps.len(), 2);
    rig.engine.cancel(both.id);

    let nothing = vec![notchling_engine::machines::node_updates::UpdateRow {
        machine: "m1".into(),
        state: UpdateState::NeedsBuild("Build the Linux nodes first".into()),
    }];
    assert!(plan(&rig.engine, &nothing, &BTreeSet::new()).is_err());
}

#[test]
fn the_button_names_the_machine_or_counts_them() {
    assert_eq!(run_label(&["m1".to_owned()]), "Update m1");
    assert_eq!(
        run_label(&["m1".to_owned(), "m2".to_owned()]),
        "Update 2 Machines"
    );
}

#[test]
fn a_machine_taken_off_the_list_is_not_selected() {
    let rig = world(&["m1", "m2"]);
    let rows = ready_rows(&rig, &["m1", "m2"]);
    let off: BTreeSet<String> = ["m1".to_owned()].into();
    assert_eq!(selected(&rows, &BTreeSet::new(), &off), ["m2"]);
}

#[test]
fn machines_that_need_nothing_are_listed_dimmed_with_the_reason() {
    let machines = [node("m1", MachineOs::MacOs)];
    let both = model(
        vec![
            member(M1, "m1", "macos", "aarch64", Some(OURS)),
            member(VPS, "vps-sing-1", "linux", "x86_64", Some(OURS)),
        ],
        json!([]),
    );
    let got = rows(&both, &machines);
    assert!(got.is_empty());
    let idle = idle_lines(&both, &got);
    let by: BTreeMap<&str, (&str, Kind)> = idle
        .iter()
        .map(|l| (l.machine.as_str(), (l.text.as_str(), l.kind)))
        .collect();
    assert_eq!(
        by["m5"],
        ("This Mac. The others are updated from it.", Kind::Idle)
    );
    assert_eq!(by["m1"], ("Already up to date.", Kind::Idle));
    assert_eq!(by["vps-sing-1"], ("Already up to date.", Kind::Idle));

    let old = model(
        vec![member(M1, "m1", "macos", "aarch64", Some(OTHER))],
        json!([]),
    );
    let got = rows(&old, &machines);
    let idle = idle_lines(&old, &got);
    assert!(idle.iter().all(|l| l.machine != "m1"), "m1 is a real row");
    assert!(idle.iter().any(|l| l.machine == "m5"));
}
