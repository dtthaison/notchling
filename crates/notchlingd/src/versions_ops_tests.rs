use crate::acting_ops_tests::FakeRoom;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use crate::versions_ops::{Clock, VersionsOps, SEEN_DOC, WORKERS_DOC};
use notchling_engine::control::launcher::{FakeLauncher, LaunchOutput};
use notchling_engine::store;
use notchling_engine::versions::mac::Roots;
use notchling_engine::versions::record::{Fact, MemoryRecorder};
use notchling_engine::versions::{Tool, Version};
use notchling_local_api::{ops, topics, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

struct Rig {
    tmp: TempDir,
    ops: Arc<VersionsOps>,
    room: Arc<FakeRoom>,
    launcher: Arc<FakeLauncher>,
    recorder: Arc<MemoryRecorder>,
    now: Arc<AtomicI64>,
}

fn caller(tier: Tier) -> Caller {
    Caller {
        tier,
        client: if tier == Tier::App {
            ClientKind::App
        } else {
            ClientKind::Cli
        },
        peer: None,
        pid: None,
    }
}

fn rig(tag: &str) -> Rig {
    let tmp = TempDir::new(tag);
    let room = Arc::new(FakeRoom::default());
    let launcher = Arc::new(FakeLauncher::new());
    let recorder = Arc::new(MemoryRecorder::new());
    let now = Arc::new(AtomicI64::new(1_000));
    let clock: Clock = {
        let now = now.clone();
        Arc::new(move || now.load(Ordering::SeqCst))
    };
    tmp.write("own-binary", "the daemon");
    let ops = VersionsOps::new(
        Roots::inside(&tmp.path("home")),
        tmp.path("data"),
        room.clone(),
        launcher.clone(),
        recorder.clone(),
        clock,
        Version::parse("0.1.0").unwrap(),
    );
    ops.set_exe(Some(tmp.path("own-binary")));
    Rig {
        tmp,
        ops,
        room,
        launcher,
        recorder,
        now,
    }
}

#[cfg(unix)]
fn native_claude(rig: &Rig, version: &str) {
    rig.tmp.write(
        &format!("home/.local/share/claude/versions/{version}"),
        &format!("claude {version}"),
    );
    let link = rig.tmp.path("home/.local/bin/claude");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(
        rig.tmp
            .path(&format!("home/.local/share/claude/versions/{version}")),
        link,
    )
    .unwrap();
}

fn get(rig: &Rig, args: Value) -> Value {
    rig.ops
        .call(&caller(Tier::Agent), ops::VERSIONS_GET, &args)
        .unwrap()
}

fn row(reply: &Value, tool: &str) -> Value {
    reply["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["tool"] == tool)
        .cloned()
        .unwrap()
}

#[test]
fn get_is_agent_tier() {
    let rig = rig("versions-tier");
    assert_eq!(rig.ops.spec(ops::VERSIONS_GET), Some(OpSpec::AGENT));
    let reply = get(&rig, json!({}));
    assert_eq!(reply["tools"].as_array().unwrap().len(), 4);
    assert_eq!(rig.ops.spec(ops::SESSIONS_LIST), None);
    assert_eq!(rig.ops.topics(), &[topics::VERSIONS]);
}

#[test]
fn workers_is_app_tier() {
    let rig = rig("versions-workers-tier");
    let spec = rig.ops.spec(ops::VERSIONS_WORKERS).unwrap();
    assert_eq!(spec.tier, Tier::App);
    assert!(!spec.approval);
}

#[cfg(unix)]
#[test]
fn get_reads_a_native_layout() {
    let rig = rig("versions-native");
    native_claude(&rig, "2.1.324");
    let reply = get(&rig, json!({}));
    let claude = row(&reply, "claude");
    assert_eq!(claude["installed"]["version"], "2.1.324");
    assert_eq!(claude["installed"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(claude["method"], "native");
    let notchling = row(&reply, "notchling");
    assert_eq!(notchling["installed"]["version"], "0.1.0");
    assert_eq!(notchling["method"], "release");
    assert_eq!(notchling["installed"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(row(&reply, "codex")["installed"], Value::Null);
    assert!(rig.launcher.calls().is_empty());
}

#[test]
fn get_has_the_full_shape_with_empty_reference() {
    let rig = rig("versions-shape");
    let reply = get(&rig, json!({"tool": "codex"}));
    let rows = reply["tools"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let codex = &rows[0];
    for key in [
        "tool",
        "installed",
        "method",
        "path",
        "problems",
        "running",
        "reference",
        "usable",
        "failed",
        "check",
    ] {
        assert!(codex.get(key).is_some(), "{key}");
    }
    assert_eq!(codex["reference"], Value::Null);
    assert_eq!(codex["usable"], json!(false));
    assert_eq!(codex["check"], Value::Null);
    assert_eq!(codex["failed"], json!([]));
    assert_eq!(codex["running"], json!([]));
}

#[test]
fn a_tool_can_be_asked_for_by_either_name_for_antigravity() {
    let rig = rig("versions-names");
    for name in ["agy", "antigravity"] {
        let reply = get(&rig, json!({"tool": name}));
        assert_eq!(reply["tools"][0]["tool"], "antigravity");
    }
    let err = rig
        .ops
        .call(
            &caller(Tier::Agent),
            ops::VERSIONS_GET,
            &json!({"tool": "gemini"}),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    let err = rig
        .ops
        .call(&caller(Tier::Agent), ops::VERSIONS_GET, &json!({"tool": 7}))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
}

#[cfg(unix)]
#[test]
fn rescan_after_a_link_moves_publishes_changed() {
    let rig = rig("versions-moved");
    native_claude(&rig, "2.1.324");
    rig.ops.rescan();
    assert!(rig.room.named(topics::VERSIONS_CHANGED).is_empty());
    native_claude(&rig, "2.1.325");
    rig.ops.rescan();
    let events = rig.room.named(topics::VERSIONS_CHANGED);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["tool"], "claude");
    assert_eq!(events[0]["installed"]["version"], "2.1.325");
    let changed: Vec<Fact> = rig
        .recorder
        .facts()
        .into_iter()
        .filter(|f| matches!(f, Fact::Changed { .. }))
        .collect();
    assert_eq!(changed.len(), 1);
}

#[cfg(unix)]
#[test]
fn unchanged_rescan_publishes_nothing_and_hashes_nothing() {
    let rig = rig("versions-unchanged");
    native_claude(&rig, "2.1.324");
    rig.ops.rescan();
    let hashed = rig.ops.hashed();
    assert!(hashed >= 2);
    rig.ops.rescan();
    rig.ops.rescan();
    assert_eq!(rig.ops.hashed(), hashed);
    assert!(rig.room.named(topics::VERSIONS_CHANGED).is_empty());
}

#[test]
fn agy_version_asked_at_most_every_ten_minutes() {
    let rig = rig("versions-agy");
    rig.tmp.write("home/.local/bin/agy", "binary");
    for _ in 0..3 {
        rig.launcher
            .push_output(Ok(LaunchOutput::ok("agy 1.4.2\n")));
    }
    rig.ops.rescan();
    rig.ops.rescan();
    rig.now.fetch_add(599_000, Ordering::SeqCst);
    rig.ops.rescan();
    assert_eq!(rig.launcher.commands().len(), 1);
    let reply = get(&rig, json!({"tool": "agy"}));
    assert_eq!(reply["tools"][0]["installed"]["version"], "1.4.2");
    rig.now.fetch_add(1_000, Ordering::SeqCst);
    rig.ops.rescan();
    assert_eq!(rig.launcher.commands().len(), 2);
}

#[test]
fn a_first_get_never_runs_agy() {
    let rig = rig("versions-agy-get");
    rig.tmp.write("home/.local/bin/agy", "binary");
    let reply = get(&rig, json!({"tool": "agy"}));
    assert_eq!(reply["tools"][0]["installed"]["version"], Value::Null);
    assert_eq!(reply["tools"][0]["method"], "standalone");
    assert!(rig.launcher.calls().is_empty());
}

#[test]
fn fixture_root_runs_only_its_own_agy() {
    let rig = rig("versions-agy-fixture");
    rig.tmp.write("home/.local/bin/agy", "binary");
    rig.launcher
        .push_output(Ok(LaunchOutput::ok("agy 1.4.2\n")));
    rig.ops.rescan();
    let commands = rig.launcher.commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].argv,
        vec![
            rig.tmp
                .path("home/.local/bin/agy")
                .to_string_lossy()
                .into_owned(),
            "--version".to_string()
        ]
    );
}

#[test]
fn a_failed_agy_answer_is_cant_tell_and_not_asked_again_soon() {
    let rig = rig("versions-agy-bad");
    rig.tmp.write("home/.local/bin/agy", "binary");
    rig.launcher
        .push_output(Ok(LaunchOutput::ok("agy 1.4.2; rm -rf ~\n")));
    rig.ops.rescan();
    rig.ops.rescan();
    assert_eq!(rig.launcher.commands().len(), 1);
    let reply = get(&rig, json!({"tool": "agy"}));
    assert_eq!(reply["tools"][0]["installed"]["version"], Value::Null);
    assert_eq!(
        reply["tools"][0]["problems"],
        json!([{"kind": "cant_tell"}])
    );
}

#[cfg(unix)]
#[test]
fn first_sighting_records_seen_once() {
    let rig = rig("versions-seen");
    native_claude(&rig, "2.1.324");
    rig.ops.rescan();
    rig.ops.rescan();
    let seen: Vec<(Tool, String)> = rig
        .recorder
        .facts()
        .into_iter()
        .filter_map(|f| match f {
            Fact::Seen { tool, version } => Some((tool, version.to_string())),
            _ => None,
        })
        .collect();
    assert_eq!(
        seen.iter().filter(|(t, _)| *t == Tool::Claude).count(),
        1,
        "{seen:?}"
    );
    assert!(seen.contains(&(Tool::Claude, "2.1.324".to_string())));
    assert!(seen.contains(&(Tool::Notchling, "0.1.0".to_string())));
    let doc = store::doc(&rig.tmp.path("data"), SEEN_DOC)
        .unwrap()
        .unwrap();
    assert!(doc.contains("claude:2.1.324"));
}

#[test]
fn workers_come_from_the_doc_and_hold_only_typed_facts() {
    let rig = rig("versions-workers");
    assert_eq!(
        rig.ops
            .call(&caller(Tier::App), ops::VERSIONS_WORKERS, &json!({}))
            .unwrap(),
        json!({"workers": {}})
    );
    let doc = json!({
        "m1": {
            "tools": {"claude": {"state": "installed", "version": "2.1.324", "method": "native", "path": "/h/.local/bin/claude"},
                      "agy": {"state": "notInstalled"}},
            "atMs": 5,
            "extra": "dropped"
        },
        "junk": {"tools": "not a map"}
    });
    store::put_doc(&rig.tmp.path("data"), WORKERS_DOC, &doc.to_string()).unwrap();
    let reply = rig
        .ops
        .call(&caller(Tier::App), ops::VERSIONS_WORKERS, &json!({}))
        .unwrap();
    let workers = reply["workers"].as_object().unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers["m1"]["atMs"], 5);
    assert_eq!(workers["m1"]["tools"]["claude"]["version"], "2.1.324");
    assert_eq!(
        workers["m1"]["tools"]["antigravity"]["state"],
        "notInstalled"
    );
    assert!(workers["m1"].get("extra").is_none());
}

#[test]
fn start_scans_and_close_stops_the_thread() {
    let rig = rig("versions-thread");
    rig.tmp.write("home/.local/bin/agy", "binary");
    rig.ops.start();
    let begun = std::time::Instant::now();
    while rig.launcher.commands().is_empty() && begun.elapsed().as_secs() < 10 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(rig.launcher.commands().len(), 1);
    rig.ops.close();
}
