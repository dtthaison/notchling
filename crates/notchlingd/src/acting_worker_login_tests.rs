use crate::acting_ops_tests::{caller, sessions, setup_in, Setup, PROMPT};
use crate::cluster_logins::LoginEnv;
use notchling_engine::cluster::grant_files::GrantValue;
use notchling_engine::control::{FakeLauncher, LaunchOutput, Launcher};
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, ApiError, Caller, ClientKind, Tier};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

const SIGNED_OUT_STATUS: &str = "{\n  \"loggedIn\": false,\n  \"authMethod\": \"none\",\n  \"apiProvider\": \"firstParty\"\n}\n";
const SIGNED_IN_STATUS: &str = "{\"loggedIn\": true, \"authMethod\": \"claude.ai\"}\n";
const NOT_LOGGED_IN: &str = "Not logged in · Please run /login";

struct Standing;

impl LoginEnv for Standing {
    fn token(&self) -> Option<GrantValue> {
        Some(GrantValue::new("NL-MARKER-831".to_string()))
    }

    fn standing(&self) -> bool {
        true
    }

    fn launched(&self, _session: &str, _token: &GrantValue) -> Result<(), String> {
        Ok(())
    }
}

fn worker(outputs: Vec<LaunchOutput>, login: bool) -> (Setup, Arc<FakeLauncher>) {
    let fake = Arc::new(FakeLauncher::new());
    for out in outputs {
        fake.push_output(Ok(out));
    }
    let launcher: Arc<dyn Launcher> = fake.clone();
    let roots = Roots {
        main_dir: PathBuf::from("/nonexistent/claude"),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let s = setup_in(true, PROMPT, sessions(), roots, Some(launcher));
    if login {
        s.ops.attach_login(Arc::new(Standing));
    }
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

fn start(s: &Setup, who: &Caller) -> Result<serde_json::Value, ApiError> {
    s.ops
        .call(who, ops::SESSION_NEW, &json!({"folder": "/w/api"}))
}

fn reason_of(e: &ApiError) -> Option<String> {
    e.data
        .as_ref()
        .and_then(|d| d["reason"].as_str())
        .map(str::to_string)
}

#[test]
fn a_session_the_cluster_cannot_start_on_a_signed_out_claude_says_so() {
    let (s, fake) = worker(
        vec![
            LaunchOutput::failed(1, NOT_LOGGED_IN),
            LaunchOutput::failed(1, SIGNED_OUT_STATUS),
        ],
        false,
    );
    let err = start(&s, &node()).unwrap_err();
    assert_eq!(reason_of(&err).as_deref(), Some("worker_claude_signed_out"));
    assert_eq!(
        err.data.as_ref().unwrap()["wire"],
        "worker_claude_signed_out"
    );
    assert!(err.message.contains("signed in"), "{}", err.message);
    assert_eq!(fake.commands().len(), 2);
}

#[test]
fn a_failure_on_a_signed_in_claude_keeps_its_own_message() {
    let (s, _fake) = worker(
        vec![
            LaunchOutput::failed(1, "Workspace trust is needed"),
            LaunchOutput::ok(SIGNED_IN_STATUS),
        ],
        false,
    );
    let err = start(&s, &node()).unwrap_err();
    assert_eq!(reason_of(&err), None);
    assert!(err.message.contains("Workspace trust"), "{}", err.message);
}

#[test]
fn a_session_started_with_the_cluster_login_or_from_the_app_is_not_probed() {
    let (s, fake) = worker(vec![LaunchOutput::failed(1, NOT_LOGGED_IN)], true);
    let err = start(&s, &node()).unwrap_err();
    assert_eq!(reason_of(&err), None);
    assert_eq!(fake.commands().len(), 1);
    let (s, fake) = worker(vec![LaunchOutput::failed(1, NOT_LOGGED_IN)], false);
    let err = start(&s, &caller(ClientKind::App)).unwrap_err();
    assert_eq!(reason_of(&err), None);
    assert_eq!(fake.commands().len(), 1);
}
