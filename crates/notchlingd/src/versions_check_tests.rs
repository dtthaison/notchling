use crate::acting_ops_tests::FakeRoom;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use crate::versions_ops::{Clock, VersionsOps};
use notchling_engine::control::launcher::{Call, FakeLauncher, LaunchError, LaunchOutput};
use notchling_engine::versions::mac::Roots;
use notchling_engine::versions::record::{Fact, MemoryRecorder};
use notchling_engine::versions::Version;
use notchling_local_api::{ops, topics, Caller, ClientKind, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

struct Rig {
    tmp: TempDir,
    ops: Arc<VersionsOps>,
    room: Arc<FakeRoom>,
    launcher: Arc<FakeLauncher>,
    recorder: Arc<MemoryRecorder>,
    now: Arc<AtomicI64>,
}

fn app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

fn agent() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
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
fn native_claude(rig: &Rig, version: &str, body: &str) {
    let target = rig
        .tmp
        .path(&format!("home/.local/share/claude/versions/{version}"));
    rig.tmp.write(
        &format!("home/.local/share/claude/versions/{version}"),
        body,
    );
    let link = rig.tmp.path("home/.local/bin/claude");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(target, link).unwrap();
}

fn codex_standalone(rig: &Rig, version: &str) {
    rig.tmp.write(
        "home/.codex/packages/standalone/pkg/codex-package.json",
        &format!("{{\"version\":\"{version}\"}}"),
    );
    rig.tmp
        .write("home/.codex/packages/standalone/pkg/codex", "codex binary");
}

fn claude_transcripts(rig: &Rig) {
    rig.tmp.write(
        "home/.claude/projects/proj/s1.jsonl",
        "{\"type\":\"user\",\"version\":\"2.1.324\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n{\"type\":\"system\",\"version\":\"2.1.324\",\"subtype\":\"turn_duration\",\"durationMs\":10}\n",
    );
}

fn out(text: &str) -> Result<LaunchOutput, LaunchError> {
    Ok(LaunchOutput::ok(text))
}

fn get(rig: &Rig, tool: &str) -> Value {
    let reply = rig
        .ops
        .call(&agent(), ops::VERSIONS_GET, &json!({ "tool": tool }))
        .unwrap();
    reply["tools"][0].clone()
}

fn check(rig: &Rig, tool: &str) -> Value {
    rig.ops
        .call(&app(), ops::VERSIONS_CHECK, &json!({ "tool": tool }))
        .unwrap()
}

fn ready(rig: &Rig) {
    rig.ops.rescan();
    rig.ops.checks().set_enabled(true);
}

fn tree(root: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, into: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            if meta.file_type().is_symlink() {
                let target = std::fs::read_link(&path).unwrap();
                into.insert(name, target.to_string_lossy().as_bytes().to_vec());
            } else if meta.is_dir() {
                into.insert(name.clone(), Vec::new());
                walk(&path, root, into);
            } else {
                into.insert(name, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut into = BTreeMap::new();
    walk(root, root, &mut into);
    into
}

fn argvs(rig: &Rig) -> Vec<Vec<String>> {
    rig.launcher
        .commands()
        .into_iter()
        .map(|c| c.argv)
        .collect()
}

#[test]
fn check_is_app_tier() {
    let rig = rig("check-tier");
    assert_eq!(rig.ops.spec(ops::VERSIONS_CHECK), Some(OpSpec::APP));
}

#[cfg(unix)]
#[test]
fn check_runs_only_one_shot_commands() {
    let rig = rig("check-oneshot");
    native_claude(&rig, "2.1.324", "claude 2.1.324");
    claude_transcripts(&rig);
    ready(&rig);
    rig.launcher.push_output(out("2.1.324 (Claude Code)\n"));
    rig.launcher.push_output(out("Auto-update: enabled\n"));
    assert_eq!(check(&rig, "claude")["started"], true);
    rig.ops.wait_checks();
    let bin = rig.tmp.path("home/.local/bin/claude");
    let bin = bin.to_string_lossy().into_owned();
    assert_eq!(
        argvs(&rig),
        vec![
            vec![bin.clone(), "--version".to_string()],
            vec![bin, "doctor".to_string()],
        ]
    );
    assert!(rig
        .launcher
        .calls()
        .iter()
        .all(|c| matches!(c, Call::Run(..))));
}

#[cfg(unix)]
#[test]
fn check_writes_nothing_under_home() {
    let rig = rig("check-nowrite");
    native_claude(&rig, "2.1.324", "claude 2.1.324");
    claude_transcripts(&rig);
    ready(&rig);
    let before = tree(&rig.tmp.path("home"));
    rig.launcher.push_output(out("2.1.324 (Claude Code)\n"));
    rig.launcher.push_output(out("Auto-update: enabled\n"));
    check(&rig, "claude");
    rig.ops.wait_checks();
    assert_eq!(get(&rig, "claude")["reference"]["partly"], true);
    assert_eq!(tree(&rig.tmp.path("home")), before);
}

#[cfg(unix)]
#[test]
fn passing_check_sets_a_partly_checked_reference() {
    let rig = rig("check-pass");
    native_claude(&rig, "2.1.324", "claude 2.1.324");
    claude_transcripts(&rig);
    ready(&rig);
    rig.launcher.push_output(out("2.1.324 (Claude Code)\n"));
    rig.launcher.push_output(out("Auto-update: enabled\n"));
    check(&rig, "claude");
    rig.ops.wait_checks();
    let row = get(&rig, "claude");
    assert_eq!(row["reference"]["result"], "partly");
    assert_eq!(row["reference"]["version"], "2.1.324");
    assert_eq!(
        row["reference"]["unchecked"],
        json!(["screens", "dialogs", "turns"])
    );
    assert_eq!(row["usable"], true);
    assert_eq!(row["check"]["result"], "partly");
    let events = rig.room.named(topics::VERSIONS_CHECKED);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["version"], "2.1.324");
    assert!(rig
        .recorder
        .facts()
        .iter()
        .any(|f| matches!(f, Fact::Checked { .. })));
}

#[test]
fn failing_help_marks_the_version_failed() {
    let rig = rig("check-fail");
    codex_standalone(&rig, "0.166.0");
    ready(&rig);
    rig.launcher.push_output(out("codex-cli 0.166.0\n"));
    rig.launcher
        .push_output(out("Usage: codex exec [OPTIONS]\n  -m, --model\n"));
    check(&rig, "codex");
    rig.ops.wait_checks();
    let row = get(&rig, "codex");
    assert_eq!(row["reference"], Value::Null);
    assert_eq!(row["usable"], false);
    assert_eq!(row["failed"][0]["version"], "0.166.0");
    assert_eq!(row["failed"][0]["part"], "exec_flags");
    assert_eq!(row["check"]["result"], "failed");
    assert!(row["check"]["why"].as_str().unwrap().contains("--json"));
    check(&rig, "codex");
    rig.ops.wait_checks();
    rig.ops.rescan();
    rig.ops.wait_checks();
    assert_eq!(
        argvs(&rig).len(),
        4,
        "a rescan must not check a failed version"
    );
}

#[test]
fn cant_tell_retries_once_then_fails() {
    let rig = rig("check-cant");
    codex_standalone(&rig, "0.166.0");
    ready(&rig);
    for _ in 0..2 {
        rig.launcher.push_output(Err(LaunchError::Timeout(10)));
    }
    check(&rig, "codex");
    rig.ops.wait_checks();
    assert_eq!(get(&rig, "codex")["check"]["result"], "cant_tell");
    assert_eq!(get(&rig, "codex")["failed"], json!([]));
    rig.ops.retry_due_checks();
    rig.ops.wait_checks();
    assert_eq!(argvs(&rig).len(), 2, "not due yet");
    rig.now.fetch_add(120_000, Ordering::SeqCst);
    for _ in 0..2 {
        rig.launcher
            .push_output(Err(LaunchError::Spawn("gone".into())));
    }
    rig.ops.retry_due_checks();
    rig.ops.wait_checks();
    let row = get(&rig, "codex");
    assert_eq!(row["failed"][0]["part"], "version");
    assert_eq!(row["failed"][0]["why"], "it didn't start");
    assert_eq!(row["check"]["result"], "failed");
    rig.now.fetch_add(120_000, Ordering::SeqCst);
    rig.ops.retry_due_checks();
    rig.ops.wait_checks();
    assert_eq!(argvs(&rig).len(), 4, "only one retry");
}

#[cfg(unix)]
#[test]
fn new_binary_same_version_rechecks() {
    let rig = rig("check-newbin");
    native_claude(&rig, "2.1.324", "claude 2.1.324 first build");
    claude_transcripts(&rig);
    rig.ops.checks().set_enabled(true);
    for _ in 0..2 {
        rig.launcher.push_output(out("2.1.324 (Claude Code)\n"));
        rig.launcher.push_output(out("Auto-update: enabled\n"));
    }
    rig.ops.rescan();
    rig.ops.wait_checks();
    assert_eq!(argvs(&rig).len(), 2);
    rig.ops.rescan();
    rig.ops.wait_checks();
    assert_eq!(argvs(&rig).len(), 2, "same binary is not checked twice");
    native_claude(&rig, "2.1.324", "claude 2.1.324 second build");
    rig.ops.rescan();
    rig.ops.wait_checks();
    assert_eq!(argvs(&rig).len(), 4);
    assert_eq!(get(&rig, "claude")["usable"], true);
}

#[cfg(unix)]
#[test]
fn second_check_while_running_is_not_started() {
    let rig = rig("check-twice");
    native_claude(&rig, "2.1.324", "claude 2.1.324");
    ready(&rig);
    rig.launcher.set_delay(Duration::from_millis(300));
    assert_eq!(check(&rig, "claude")["started"], true);
    let second = check(&rig, "claude");
    assert_eq!(second, json!({ "started": false, "running": true }));
    assert_eq!(get(&rig, "claude")["check"], json!({ "running": true }));
    rig.ops.wait_checks();
}

#[test]
fn notchling_checks_itself_in_process() {
    let rig = rig("check-self");
    ready(&rig);
    assert_eq!(check(&rig, "notchling")["started"], true);
    rig.ops.wait_checks();
    assert!(rig.launcher.calls().is_empty());
    let row = get(&rig, "notchling");
    assert_eq!(row["reference"]["version"], "0.1.0");
    assert_eq!(row["usable"], true);
}

#[test]
fn a_failing_self_test_fails_notchling() {
    let rig = rig("check-self-bad");
    ready(&rig);
    rig.ops.checks().set_self_test(Arc::new(|| false));
    check(&rig, "notchling");
    rig.ops.wait_checks();
    let row = get(&rig, "notchling");
    assert_eq!(row["failed"][0]["part"], "self_test");
    assert_eq!(row["usable"], false);
}

#[test]
fn checks_are_off_until_enabled() {
    let rig = rig("check-off");
    rig.ops.rescan();
    assert_eq!(
        check(&rig, "notchling"),
        json!({ "started": false, "running": false })
    );
    rig.ops.wait_checks();
    assert!(rig.room.named(topics::VERSIONS_CHECKED).is_empty());
}

#[test]
fn an_unknown_tool_is_a_bad_request() {
    let rig = rig("check-badtool");
    let err = rig
        .ops
        .call(&app(), ops::VERSIONS_CHECK, &json!({ "tool": "emacs" }));
    assert!(err.is_err());
    let err = rig.ops.call(&app(), ops::VERSIONS_CHECK, &json!({}));
    assert!(err.is_err());
}
