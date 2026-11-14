use crate::op_group::OpGroup;
use crate::switches::{SwitchFlag, INSTALL, INSTALL_OFF};
use crate::versions_check_tests::{native_claude, out, rig, Rig};
use notchling_engine::control::launcher::{LaunchError, LaunchOutput};
use notchling_engine::store;
use notchling_engine::versions::record::{Cause, Fact};
use notchling_engine::versions::Tool;
use notchling_local_api::{ops, topics, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

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

fn switch(rig: &Rig, on: bool) {
    rig.ops
        .set_install_switch(SwitchFlag::in_memory(INSTALL, INSTALL_OFF, on));
}

fn update(rig: &Rig, args: Value) -> Result<Value, notchling_local_api::ApiError> {
    rig.ops.call(&app(), ops::VERSIONS_UPDATE_LOCAL, &args)
}

fn get(rig: &Rig, tool: &str) -> Value {
    rig.ops
        .call(&agent(), ops::VERSIONS_GET, &json!({ "tool": tool }))
        .unwrap()["tools"][0]
        .clone()
}

fn seen(rig: &Rig, entries: &[&str]) {
    store::put_doc(
        &rig.tmp.path("data"),
        "versions/seen",
        &serde_json::to_string(entries).unwrap(),
    )
    .unwrap();
}

fn claude_bin(rig: &Rig) -> String {
    rig.tmp
        .path("home/.local/bin/claude")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn update_and_latest_are_app_tier() {
    let rig = rig("update-tier");
    assert_eq!(rig.ops.spec(ops::VERSIONS_UPDATE_LOCAL), Some(OpSpec::APP));
    assert_eq!(rig.ops.spec(ops::VERSIONS_LATEST), Some(OpSpec::APP));
    let e = rig.ops.call(
        &agent(),
        ops::VERSIONS_UPDATE_LOCAL,
        &json!({"tool": "claude"}),
    );
    assert!(e.is_err() || rig.ops.spec(ops::VERSIONS_UPDATE_LOCAL) == Some(OpSpec::APP));
}

#[cfg(unix)]
#[test]
fn update_refused_with_install_off() {
    let rig = rig("update-off");
    native_claude(&rig, "2.1.324", "claude 2.1.324");
    rig.ops.rescan();
    let before = rig.launcher.calls().len();
    let e = update(&rig, json!({"tool": "claude"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert_eq!(e.message, INSTALL_OFF);
    switch(&rig, false);
    let e = update(&rig, json!({"tool": "claude"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden);
    rig.ops.wait_updates();
    assert_eq!(rig.launcher.calls().len(), before);
    assert!(rig.room.named(topics::VERSIONS_UPDATED).is_empty());
}

#[cfg(unix)]
#[test]
fn update_runs_the_methods_argv() {
    let rig = rig("update-run");
    native_claude(&rig, "2.1.324", "claude 2.1.324");
    rig.ops.rescan();
    switch(&rig, true);
    rig.launcher.push_output(out("updated"));
    assert_eq!(
        update(&rig, json!({"tool": "claude"})).unwrap()["started"],
        true
    );
    rig.ops.wait_updates();
    let commands = rig.launcher.commands();
    assert_eq!(
        commands.last().unwrap().argv,
        vec![claude_bin(&rig), "update".to_string()]
    );
    let events = rig.room.named(topics::VERSIONS_UPDATED);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["tool"], "claude");
    assert_eq!(events[0]["from"], "2.1.324");
    assert_eq!(events[0]["ok"], true);
}

#[cfg(unix)]
#[test]
fn a_failed_update_says_so() {
    let rig = rig("update-fail");
    native_claude(&rig, "2.1.324", "claude 2.1.324");
    rig.ops.rescan();
    switch(&rig, true);
    rig.launcher
        .push_output(Ok(LaunchOutput::failed(1, "boom")));
    update(&rig, json!({"tool": "claude"})).unwrap();
    rig.ops.wait_updates();
    assert_eq!(rig.room.named(topics::VERSIONS_UPDATED)[0]["ok"], false);
}

#[cfg(unix)]
#[test]
fn the_daemon_run_names_the_cause() {
    let rig = rig("update-cause");
    native_claude(&rig, "2.1.324", "claude 2.1.324");
    rig.ops.rescan();
    switch(&rig, true);
    update(&rig, json!({"tool": "claude"})).unwrap();
    rig.ops.wait_updates();
    native_claude(&rig, "2.1.325", "claude 2.1.325");
    rig.ops.rescan();
    let cause = rig.recorder.facts().into_iter().find_map(|f| match f {
        Fact::Changed {
            tool: Tool::Claude,
            cause,
            ..
        } => Some(cause),
        _ => None,
    });
    assert_eq!(cause, Some(Cause::UserUpdate));
}

#[cfg(unix)]
#[test]
fn go_back_only_for_exact_methods() {
    let rig = rig("update-back");
    native_claude(&rig, "2.1.324", "claude 2.1.324");
    seen(
        &rig,
        &[
            "claude:2.1.322",
            "claude:2.1.323",
            "claude:2.1.324",
            "codex:0.166.0",
        ],
    );
    rig.ops.rescan();
    switch(&rig, true);
    assert_eq!(get(&rig, "claude")["goBack"], "2.1.323");
    let e = update(&rig, json!({"tool": "claude", "to": "2.1.320"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    let e = update(&rig, json!({"tool": "claude", "to": "latest"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    rig.launcher.push_output(out("ok"));
    update(&rig, json!({"tool": "claude", "to": "2.1.323"})).unwrap();
    rig.ops.wait_updates();
    assert_eq!(
        rig.launcher.commands().last().unwrap().argv,
        vec![
            claude_bin(&rig),
            "install".to_string(),
            "2.1.323".to_string()
        ]
    );
    native_claude(&rig, "2.1.323", "claude 2.1.323");
    rig.ops.rescan();
    let cause = rig.recorder.facts().into_iter().find_map(|f| match f {
        Fact::Changed { cause, .. } => Some(cause),
        _ => None,
    });
    assert_eq!(cause, Some(Cause::Rollback));
}

#[cfg(unix)]
#[test]
fn brew_cannot_go_back() {
    let rig = rig("update-brew");
    rig.tmp.write(
        "home/opt/homebrew/Caskroom/claude-code/2.1.324/claude",
        "claude",
    );
    seen(&rig, &["claude:2.1.323", "claude:2.1.324"]);
    rig.ops.rescan();
    switch(&rig, true);
    let row = get(&rig, "claude");
    assert_eq!(row["method"], "brew");
    assert_eq!(row["goBack"], Value::Null);
    let e = update(&rig, json!({"tool": "claude", "to": "2.1.323"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
}

#[test]
fn latest_is_cached_for_an_hour() {
    let rig = rig("latest-cache");
    rig.launcher.push_output(out("2.1.325\n"));
    let call = |rig: &Rig| {
        rig.ops
            .call(&app(), ops::VERSIONS_LATEST, &json!({"tool": "claude"}))
            .unwrap()
    };
    let first = call(&rig);
    assert_eq!(first["latest"], "2.1.325");
    let again = call(&rig);
    assert_eq!(again["latest"], "2.1.325");
    assert_eq!(rig.launcher.commands().len(), 1);
    assert_eq!(
        rig.launcher.commands()[0].argv,
        vec![
            "curl",
            "--proto",
            "=https",
            "-fsSL",
            "--max-time",
            "10",
            "https://downloads.claude.ai/claude-code-releases/latest"
        ]
    );
    rig.now.fetch_add(3_600_001, Ordering::SeqCst);
    rig.launcher.push_output(out("2.1.326\n"));
    assert_eq!(call(&rig)["latest"], "2.1.326");
    assert_eq!(rig.launcher.commands().len(), 2);
    assert_eq!(get(&rig, "claude")["latest"], "2.1.326");
}

#[test]
fn latest_error_names_only_the_host() {
    let rig = rig("latest-error");
    rig.launcher
        .push_output(Ok(LaunchOutput::failed(22, "PLANTED-MARKER-BODY")));
    let reply = rig
        .ops
        .call(&app(), ops::VERSIONS_LATEST, &json!({"tool": "claude"}))
        .unwrap();
    assert_eq!(reply["latest"], Value::Null);
    assert_eq!(reply["error"], "couldn't reach downloads.claude.ai");
    assert!(!reply.to_string().contains("PLANTED"));
    rig.launcher
        .push_output(Err(LaunchError::Spawn("no curl".into())));
    rig.now.fetch_add(300_001, Ordering::SeqCst);
    let reply = rig
        .ops
        .call(&app(), ops::VERSIONS_LATEST, &json!({"tool": "codex"}))
        .unwrap();
    assert_eq!(reply["error"], "couldn't reach registry.npmjs.org");
}

#[test]
fn a_junk_answer_is_not_a_version() {
    let rig = rig("latest-junk");
    rig.launcher.push_output(out("<html>nope</html>"));
    let reply = rig
        .ops
        .call(&app(), ops::VERSIONS_LATEST, &json!({"tool": "claude"}))
        .unwrap();
    assert_eq!(reply["latest"], Value::Null);
    assert!(reply["error"].is_string());
}

#[test]
fn notchling_has_no_latest_read() {
    let rig = rig("latest-self");
    let reply = rig
        .ops
        .call(&app(), ops::VERSIONS_LATEST, &json!({"tool": "notchling"}))
        .unwrap();
    assert_eq!(reply["latest"], Value::Null);
    assert!(reply.get("error").is_none());
    assert!(rig.launcher.commands().is_empty());
    let _ = Arc::strong_count(&rig.launcher);
}
