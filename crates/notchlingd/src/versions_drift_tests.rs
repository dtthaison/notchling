use crate::acting_ops_tests::FakeRoom;
use crate::inbox_ops::InboxOps;
use crate::limits_ops_tests::TempDir;
use crate::versions_drift::Drifts;
use notchling_engine::inbox::{ActionId, Item, Kind, State};
use notchling_engine::machines::versions_facts;
use notchling_engine::versions::record::{Fact, MemoryRecorder};
use notchling_engine::versions::selfupdate_log;
use notchling_engine::versions::{Method, Tool, ToolState, Version, WorkerTools};
use std::collections::BTreeMap;
use std::sync::Arc;

struct Rig {
    tmp: TempDir,
    drifts: Drifts,
    inbox: Arc<InboxOps>,
    recorder: Arc<MemoryRecorder>,
}

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn rig(tag: &str) -> Rig {
    let tmp = TempDir::new(tag);
    std::fs::create_dir_all(tmp.path("data")).unwrap();
    std::fs::create_dir_all(tmp.path("inbox-data")).unwrap();
    let recorder = Arc::new(MemoryRecorder::new());
    let drifts = Drifts::new(tmp.path("data"), recorder.clone(), Arc::new(|| 7_000));
    let inbox = InboxOps::with_clock(
        tmp.path("inbox-data"),
        Arc::new(FakeRoom::default()),
        Arc::new(|| 5_000),
    );
    drifts.set_inbox(inbox.clone());
    Rig {
        tmp,
        drifts,
        inbox,
        recorder,
    }
}

fn worker_runs(rig: &Rig, machine: &str, tool: Tool, version: &str) {
    let tools = WorkerTools {
        tools: [(
            tool,
            ToolState::Installed {
                version: Some(v(version)),
                method: Method::Native,
                path: "/h/bin/x".into(),
            },
        )]
        .into(),
        at_ms: 1,
    };
    versions_facts::save(&rig.tmp.path("data"), machine, &tools).unwrap();
}

fn mac(version: &str) -> BTreeMap<Tool, Version> {
    [(Tool::Codex, v(version))].into()
}

fn notices(rig: &Rig) -> Vec<Item> {
    rig.inbox
        .items()
        .into_iter()
        .filter(|i| i.kind == Kind::VersionDrift)
        .collect()
}

#[test]
fn one_notice_per_found_version() {
    let rig = rig("drift-once");
    worker_runs(&rig, "vps-sing-1", Tool::Codex, "0.167.0");
    rig.drifts.pass(&mac("0.166.0"));
    rig.drifts.pass(&mac("0.166.0"));
    let items = notices(&rig);
    assert_eq!(items.len(), 1);
    assert!(items[0].id.starts_with("version-drift:"));
    assert_eq!(items[0].state, State::Open);
    assert_eq!(items[0].actions.len(), 1);
    assert_eq!(items[0].actions[0].id, ActionId::Open);
    assert_eq!(items[0].actions[0].label, "Open Machines");
    assert!(items[0].actions[0].primary);
    assert_eq!(
        items[0].open,
        Some(serde_json::json!({"machines": "vps-sing-1"}))
    );
    let drifted = rig
        .recorder
        .facts()
        .into_iter()
        .filter(|f| matches!(f, Fact::Drift { .. }))
        .count();
    assert_eq!(drifted, 1);

    worker_runs(&rig, "vps-sing-1", Tool::Codex, "0.168.0");
    rig.drifts.pass(&mac("0.166.0"));
    let items = notices(&rig);
    assert_eq!(items.len(), 2);
    let open: Vec<&Item> = items.iter().filter(|i| i.state == State::Open).collect();
    assert_eq!(open.len(), 1);
    assert!(open[0].detail.contains("0.168.0"));
    assert_ne!(items[0].id, items[1].id);
}

#[test]
fn a_restart_does_not_raise_it_again() {
    let rig = rig("drift-restart");
    worker_runs(&rig, "vps", Tool::Codex, "0.167.0");
    rig.drifts.pass(&mac("0.166.0"));
    let again = Drifts::new(
        rig.tmp.path("data"),
        rig.recorder.clone(),
        Arc::new(|| 9_000),
    );
    again.set_inbox(rig.inbox.clone());
    again.pass(&mac("0.166.0"));
    assert_eq!(notices(&rig).len(), 1);
}

#[test]
fn matching_again_withdraws_the_notice() {
    let rig = rig("drift-match");
    worker_runs(&rig, "vps", Tool::Codex, "0.167.0");
    rig.drifts.pass(&mac("0.166.0"));
    worker_runs(&rig, "vps", Tool::Codex, "0.166.0");
    rig.drifts.pass(&mac("0.166.0"));
    let items = notices(&rig);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state, State::Withdrawn);

    worker_runs(&rig, "vps", Tool::Codex, "0.167.0");
    rig.drifts.pass(&mac("0.166.0"));
    assert_eq!(
        notices(&rig)
            .iter()
            .filter(|i| i.state == State::Open)
            .count(),
        1
    );
}

#[test]
fn notice_text_names_both_versions() {
    let rig = rig("drift-text");
    worker_runs(&rig, "vps-sing-1", Tool::Codex, "0.167.0");
    rig.drifts.pass(&mac("0.166.0"));
    let items = notices(&rig);
    assert_eq!(items[0].title, "vps-sing-1 Runs Another Codex");
    assert_eq!(
        items[0].detail,
        "vps-sing-1 runs Codex 0.167.0, the Mac 0.166.0."
    );
}

#[test]
fn no_reference_means_no_notice() {
    let rig = rig("drift-none");
    worker_runs(&rig, "vps", Tool::Codex, "0.167.0");
    rig.drifts.pass(&BTreeMap::new());
    assert!(notices(&rig).is_empty());
}

#[test]
fn self_update_facts_reach_the_recorder_once() {
    let rig = rig("drift-selflog");
    let data = rig.tmp.path("data");
    selfupdate_log::append(
        &data,
        1,
        Tool::Claude,
        "vps",
        "off",
        true,
        "Self-update off",
    )
    .unwrap();
    rig.drifts.pass(&BTreeMap::new());
    rig.drifts.pass(&BTreeMap::new());
    let seen: Vec<Fact> = rig
        .recorder
        .facts()
        .into_iter()
        .filter(|f| matches!(f, Fact::SelfUpdate { .. }))
        .collect();
    assert_eq!(seen.len(), 1);
}

#[cfg(unix)]
#[test]
fn the_ops_pass_needs_a_usable_reference() {
    use crate::versions_check_tests::{check, claude_transcripts, native_claude, out, ready, rig};
    let rig = rig("drift-ops");
    native_claude(&rig, "2.1.324", "claude 2.1.324");
    claude_transcripts(&rig);
    let inbox = InboxOps::with_clock(
        rig.tmp.path("inbox-data"),
        rig.room.clone(),
        Arc::new(|| 5_000),
    );
    rig.ops.drifts().set_inbox(inbox.clone());
    ready(&rig);
    let tools = WorkerTools {
        tools: [(
            Tool::Claude,
            ToolState::Installed {
                version: Some(v("2.1.323")),
                method: Method::Native,
                path: "/h/bin/claude".into(),
            },
        )]
        .into(),
        at_ms: 1,
    };
    versions_facts::save(&rig.tmp.path("data"), "vps", &tools).unwrap();
    rig.ops.drift_pass();
    assert!(inbox.items().is_empty());

    rig.launcher.push_output(out("2.1.324 (Claude Code)\n"));
    rig.launcher.push_output(out("Auto-update: enabled\n"));
    check(&rig, "claude");
    rig.ops.wait_checks();
    rig.ops.drift_pass();
    let items = inbox.items();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].detail,
        "vps runs Claude Code 2.1.323, the Mac 2.1.324."
    );
}
