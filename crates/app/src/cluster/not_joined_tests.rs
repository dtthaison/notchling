use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::step::{CheckResult, Detected};

use super::add_rig_tests::{status, NODE};
use super::model::ClusterModel;
use super::not_joined::{not_joined, NotJoined};
use crate::machines::data::Setup;

fn registry() -> Vec<Machine> {
    Setup::for_render(None).parts().registry.all()
}

fn names(list: &[NotJoined]) -> Vec<&str> {
    list.iter().map(|n| n.name.as_str()).collect()
}

fn nothing(_: &str) -> Option<CheckResult> {
    None
}

fn model(members: &[(&str, &str, &str, bool)]) -> ClusterModel {
    ClusterModel::from_status(&status(None, members), "a1b2c3d4e5f60718")
}

#[test]
fn not_joined_lists_m1_and_vps() {
    let machines = registry();
    let got = not_joined(&machines, &nothing, &model(&[]));
    assert_eq!(names(&got), ["m1", "vps-sing-1"]);
    let vps = &got[1];
    assert_eq!(vps.user, "agent");
    assert!(!vps.node_on);
    assert!(!vps.host.is_empty());
    assert!(vps.os.is_some() || got[0].os.is_some());
}

#[test]
fn a_member_with_the_same_name_is_not_listed() {
    let machines = registry();
    let m = model(&[("1122334455667788", "m1", "active", false)]);
    assert_eq!(names(&not_joined(&machines, &nothing, &m)), ["vps-sing-1"]);
    let pending = model(&[("1122334455667788", "m1", "pending", false)]);
    assert_eq!(
        names(&not_joined(&machines, &nothing, &pending)),
        ["vps-sing-1"],
        "a machine waiting for approval is already on the sheet"
    );
}

#[test]
fn a_removed_member_does_not_count() {
    let machines = registry();
    let m = model(&[("1122334455667788", "m1", "removed", false)]);
    assert_eq!(
        names(&not_joined(&machines, &nothing, &m)),
        ["m1", "vps-sing-1"]
    );
}

#[test]
fn the_node_id_matches_before_the_name() {
    let machines = registry();
    let by_id = |name: &str| {
        (name == "vps-sing-1").then(|| {
            CheckResult::fail("not in a cluster yet").detected(Detected {
                node: Some(NODE.into()),
                ..Detected::default()
            })
        })
    };
    let m = model(&[(NODE, "some-other-name", "active", true)]);
    assert_eq!(names(&not_joined(&machines, &by_id, &m)), ["m1"]);
    let gone = model(&[("00000000000000ff", "some-other-name", "active", true)]);
    assert_eq!(
        names(&not_joined(&machines, &by_id, &gone)),
        ["m1", "vps-sing-1"]
    );
}

#[test]
fn a_passing_joined_step_is_not_listed() {
    let machines = registry();
    let passing = |name: &str| (name == "m1").then(|| CheckResult::pass("in home"));
    assert_eq!(
        names(&not_joined(&machines, &passing, &model(&[]))),
        ["vps-sing-1"]
    );
    let failing = |_: &str| Some(CheckResult::fail("not in a cluster yet"));
    assert_eq!(
        names(&not_joined(&machines, &failing, &model(&[]))).len(),
        2
    );
}
