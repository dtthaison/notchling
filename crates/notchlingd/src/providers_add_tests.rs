use crate::providers_ops::*;
use crate::providers_ops_tests::{codex_login, no_secrets, Rig};
use notchling_engine::accounts::procs::{FakeApps, FakeProcs, ProcCall};
use notchling_local_api::{ops, ErrorCode};
use serde_json::json;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn adding_a_codex_account_reports_the_sign_in_page_then_done() {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let rig = Rig::new("add", true, procs, FakeApps::new());
    let reply = rig
        .call(ops::PROVIDERS_ADD_BEGIN, json!({"provider": "codex"}))
        .unwrap();
    assert_eq!(reply["state"], "waiting");
    let login = rig.procs.login_state();
    login.lock().unwrap().output = "Open https://auth.openai.com/oauth/authorize?z=1\n".into();
    let waiting = rig.wait_for(|e| e["reason"] == "add" && e["state"] == "waiting");
    assert_eq!(
        waiting["sign_in_url"],
        "https://auth.openai.com/oauth/authorize?z=1"
    );
    let scratch = rig
        .procs
        .calls()
        .into_iter()
        .find_map(|c| match c {
            ProcCall::SpawnLogin(s) => Some(PathBuf::from(&s.env[0].1)),
            _ => None,
        })
        .unwrap();
    std::fs::write(
        scratch.join("auth.json"),
        codex_login("c@example.com", "u3", "w3"),
    )
    .unwrap();
    login.lock().unwrap().exit = Some(0);
    let done = rig.wait_for(|e| e["reason"] == "add" && e["state"] == "done");
    no_secrets(&done);
    assert!(rig.keychain.get("Notchling Codex", "u3:w3").is_some());
    assert!(
        !rig.codex_auth().exists(),
        "the live login was never written"
    );
}

#[test]
fn an_add_flow_stops_touching_logins_the_moment_accounts_turn_off() {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let rig = Rig::new("addoff", true, procs, FakeApps::new());
    rig.call(ops::PROVIDERS_ADD_BEGIN, json!({"provider": "codex"}))
        .unwrap();
    rig.flag.set(false);
    std::thread::sleep(Duration::from_millis(60));
    let calls = rig.keychain.calls().len();
    let scratch = rig
        .procs
        .calls()
        .into_iter()
        .find_map(|c| match c {
            ProcCall::SpawnLogin(s) => Some(PathBuf::from(&s.env[0].1)),
            _ => None,
        })
        .unwrap();
    assert!(!scratch.exists());
    rig.procs.login_state().lock().unwrap().exit = Some(0);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        rig.keychain.calls().len(),
        calls,
        "no Keychain call after the switch went off"
    );
    assert!(!rig.events().iter().any(|e| e["state"] == "done"));
    assert_eq!(
        rig.call(ops::PROVIDERS_ADD_CANCEL, json!({"provider": "codex"}))
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
}

#[test]
fn a_waiting_add_flow_ends_when_accounts_turn_off_not_at_its_next_poll() {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let rig = Rig::new("addoffnow", true, procs, FakeApps::new());
    rig.ops
        .set_add_timing(Duration::from_secs(600), Duration::from_secs(900));
    rig.call(ops::PROVIDERS_ADD_BEGIN, json!({"provider": "codex"}))
        .unwrap();
    let started = Instant::now();
    rig.flag.set(false);
    let ended = rig.wait_for(|e| e["reason"] == "add" && e["state"] == "cancelled");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(ended["error"], ACCOUNTS_OFF);
    no_secrets(&ended);
    assert!(rig.procs.login_state().lock().unwrap().killed);
    let scratch: Vec<_> = std::fs::read_dir(rig.root.join("state/codex-login"))
        .map(|d| d.flatten().collect())
        .unwrap_or_default();
    assert!(scratch.is_empty(), "{scratch:?}");
    assert!(!rig
        .keychain
        .calls()
        .iter()
        .any(|c| c.starts_with("write") || c.starts_with("delete")));
}

#[test]
fn stopping_the_daemon_cancels_a_waiting_add_only_while_accounts_are_on() {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let rig = Rig::new("close", true, procs, FakeApps::new());
    rig.call(ops::PROVIDERS_ADD_BEGIN, json!({"provider": "codex"}))
        .unwrap();
    rig.ops.close();
    assert!(rig.procs.login_state().lock().unwrap().killed);

    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let off = Rig::new("closeoff", true, procs, FakeApps::new());
    off.call(ops::PROVIDERS_ADD_BEGIN, json!({"provider": "codex"}))
        .unwrap();
    off.flag.set(false);
    off.ops.close();
    assert!(!off
        .keychain
        .calls()
        .iter()
        .any(|c| c.starts_with("write") || c.starts_with("delete")));
}
