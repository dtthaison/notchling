use crate::acting_ops_tests::{caller, sessions, setup_in, Setup, PROMPT};
use crate::cluster_logins::LoginEnv;
use notchling_engine::cluster::grant_files::GrantValue;
use notchling_engine::control::{Call, FakeLauncher, LaunchOutput, Launcher, LOGIN_TOKEN_VAR};
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, Caller, ClientKind, Tier};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const MARKER: &str = "NL-MARKER-830";

struct Held(Mutex<Option<GrantValue>>);

impl LoginEnv for Held {
    fn token(&self) -> Option<GrantValue> {
        self.0.lock().unwrap().clone()
    }

    fn standing(&self) -> bool {
        self.0.lock().unwrap().is_some()
    }
}

fn rig(token: Option<&str>) -> (Setup, Arc<FakeLauncher>) {
    let fake = Arc::new(FakeLauncher::new());
    for n in 0..4 {
        fake.push_output(Ok(LaunchOutput::ok(&format!(
            "backgrounded · 6ce4c19{n} · api (idle)\n"
        ))));
    }
    let launcher: Arc<dyn Launcher> = fake.clone();
    let roots = Roots {
        main_dir: PathBuf::from("/nonexistent/claude"),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let s = setup_in(true, PROMPT, sessions(), roots, Some(launcher));
    let held = Held(Mutex::new(token.map(|t| GrantValue::new(t.to_string()))));
    s.ops.attach_login(Arc::new(held));
    (s, fake)
}

fn node() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::Node,
        peer: None,
        pid: None,
    }
}

fn start(s: &Setup, who: &Caller) {
    s.ops
        .call(who, ops::SESSION_NEW, &json!({"folder": "/w/api"}))
        .expect("started");
}

fn env_keys(fake: &FakeLauncher) -> Vec<Vec<String>> {
    fake.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Run(command, _) => Some(command.secret_env.keys().cloned().collect()),
            Call::Pty(_) => None,
        })
        .collect()
}

#[test]
fn a_session_the_cluster_starts_gets_the_login_in_its_environment_only() {
    let (s, fake) = rig(Some(MARKER));
    start(&s, &node());
    assert_eq!(env_keys(&fake), [vec![LOGIN_TOKEN_VAR.to_string()]]);
    let shown = format!("{:?}", fake.calls());
    assert!(
        !shown.contains("NL-MARKER"),
        "a recorded call held the token"
    );
    for command in fake.commands() {
        assert!(command.argv.iter().all(|a| !a.contains("NL-MARKER")));
        assert!(command
            .env
            .values()
            .flatten()
            .all(|v| !v.contains("NL-MARKER")));
    }
}

#[test]
fn a_session_started_from_the_app_or_a_local_client_never_gets_it() {
    let (s, fake) = rig(Some(MARKER));
    for client in [ClientKind::App, ClientKind::Cli, ClientKind::Hook] {
        start(&s, &caller(client));
    }
    let keys = env_keys(&fake);
    assert_eq!(keys.len(), 3);
    assert!(keys.iter().all(Vec::is_empty));
}

#[test]
fn without_a_standing_login_nothing_is_added() {
    let (s, fake) = rig(None);
    start(&s, &node());
    assert!(env_keys(&fake).iter().all(Vec::is_empty));
}

#[test]
fn a_daemon_with_no_login_attached_adds_nothing() {
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok(
        "backgrounded · 6ce4c190 · api (idle)\n",
    )));
    let launcher: Arc<dyn Launcher> = fake.clone();
    let roots = Roots {
        main_dir: PathBuf::from("/nonexistent/claude"),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let s = setup_in(true, PROMPT, sessions(), roots, Some(launcher));
    start(&s, &node());
    assert!(env_keys(&fake).iter().all(Vec::is_empty));
}
