use crate::providers_add_tests::{attempt_of, begin};
use crate::providers_ops_tests::{codex_login, Rig};
use notchling_engine::accounts::procs::{FakeApps, FakeProcs, ProcCall};
use notchling_local_api::ops;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn codex_rig(tag: &str) -> Rig {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    Rig::new(tag, true, procs, FakeApps::new())
}

fn codex_add_begins(rig: &Rig) -> (u64, PathBuf) {
    rig.procs.login_state().lock().unwrap().exit = None;
    let attempt = attempt_of(&begin(&rig.ops, "codex").unwrap());
    let scratch = rig
        .procs
        .calls()
        .into_iter()
        .rev()
        .find_map(|c| match c {
            ProcCall::SpawnLogin(s) => Some(PathBuf::from(&s.env[0].1)),
            _ => None,
        })
        .unwrap();
    (attempt, scratch)
}

fn codex_add_ends(rig: &Rig, attempt: u64, scratch: &Path, login: &[u8]) -> Option<Vec<String>> {
    std::fs::write(scratch.join("auth.json"), login).unwrap();
    rig.procs.login_state().lock().unwrap().exit = Some(0);
    rig.wait_for(|e| e["reason"] == "add" && e["state"] == "done" && e["attempt"] == attempt);
    rig.ops.added(attempt)
}

#[test]
fn a_terminal_codex_login_during_an_add_is_not_what_the_add_stored() {
    let rig = codex_rig("addterm");
    let known = codex_login("k@example.com", "u1", "w1");
    std::fs::write(rig.codex_auth(), &known).unwrap();
    rig.call(ops::PROVIDERS_LIST, json!({})).unwrap();

    let (attempt, scratch) = codex_add_begins(&rig);
    std::fs::write(rig.codex_auth(), codex_login("t@example.com", "u8", "w8")).unwrap();
    assert_eq!(
        codex_add_ends(&rig, attempt, &scratch, &known),
        Some(Vec::new()),
        "it ended on a known account, and the terminal login is not its own"
    );

    let (attempt, scratch) = codex_add_begins(&rig);
    std::fs::write(rig.codex_auth(), codex_login("v@example.com", "u9", "w9")).unwrap();
    let new = codex_login("n@example.com", "u3", "w3");
    assert_eq!(
        codex_add_ends(&rig, attempt, &scratch, &new),
        Some(vec!["u3:w3".to_string()]),
        "the new account is the one its own sign-in ended on"
    );
}

#[test]
fn a_codex_login_in_use_but_never_listed_is_known_before_an_add() {
    let rig = codex_rig("addlive");
    let live = codex_login("k@example.com", "u1", "w1");
    std::fs::write(rig.codex_auth(), &live).unwrap();
    let (attempt, scratch) = codex_add_begins(&rig);
    assert_eq!(
        codex_add_ends(&rig, attempt, &scratch, &live),
        Some(Vec::new())
    );
}

#[test]
fn an_antigravity_add_records_the_login_it_ended_on() {
    let rig = Rig::new("addagy", true, FakeProcs::new(), FakeApps::new());
    let token = rig.root.join("gemini/jetski-standalone-oauth-token");
    std::fs::create_dir_all(token.parent().unwrap()).unwrap();
    let sign_in = |email: &str| {
        let login = json!({"auth_method": "keyring", "id_token_email": email,
            "token": {"access_token": "t", "refresh_token": "r"}});
        std::fs::write(&token, login.to_string()).unwrap();
    };
    let ends = |attempt: u64| {
        rig.wait_for(|e| e["reason"] == "add" && e["state"] == "done" && e["attempt"] == attempt);
        rig.ops.added(attempt)
    };
    sign_in("a@example.com");
    let attempt = attempt_of(&begin(&rig.ops, "antigravity").unwrap());
    sign_in("b@example.com");
    assert_eq!(ends(attempt), Some(vec!["email:b@example.com".to_string()]));

    let attempt = attempt_of(&begin(&rig.ops, "antigravity").unwrap());
    sign_in("a@example.com");
    assert_eq!(ends(attempt), Some(Vec::new()), "known before it began");
}

#[test]
fn an_antigravity_add_waits_through_a_half_written_login() {
    let rig = Rig::new("addagyhalf", true, FakeProcs::new(), FakeApps::new());
    let token = rig.root.join("gemini/jetski-standalone-oauth-token");
    std::fs::create_dir_all(token.parent().unwrap()).unwrap();
    let sign_in = |email: &str| {
        let login = json!({"auth_method": "keyring", "id_token_email": email,
            "token": {"access_token": "t", "refresh_token": "r"}});
        std::fs::write(&token, login.to_string()).unwrap();
    };
    sign_in("a@example.com");
    let attempt = attempt_of(&begin(&rig.ops, "antigravity").unwrap());
    std::fs::write(&token, "{\"auth").unwrap();
    std::thread::sleep(Duration::from_millis(120));
    assert!(
        !rig.events()
            .iter()
            .any(|e| e["reason"] == "add" && e["attempt"] == attempt && e["state"] == "done"),
        "{:?}",
        rig.events()
    );
    sign_in("b@example.com");
    rig.wait_for(|e| e["reason"] == "add" && e["state"] == "done" && e["attempt"] == attempt);
    assert_eq!(
        rig.ops.added(attempt),
        Some(vec!["email:b@example.com".to_string()])
    );
}
