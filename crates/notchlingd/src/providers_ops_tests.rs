//! The provider ops over fakes in a temp folder: no Keychain, no process, no network, no real
//! `~/.codex`, `~/.gemini` or Claude Desktop folder. The off tests fail if any fake is called.

use crate::acting_ops_tests::FakeRoom;
use crate::layout::Layout;
use crate::providers_ops::*;
use crate::switches::ACCOUNTS;
use notchling_engine::accounts::antigravity::{
    AntigravityDeps, AntigravityPaths, AntigravityStore,
};
use notchling_engine::accounts::codex::{CodexDeps, CodexPaths, CodexStore};
use notchling_engine::accounts::desktop::{DesktopDeps, DesktopPaths, DesktopStore};
use notchling_engine::accounts::generic_password::{FakeKeychain, Keychain};
use notchling_engine::accounts::procs::{FakeApps, FakeProcs, ProcCall};
use notchling_engine::providers::{
    AccountError, AccountProvider, AddProgress, AddStarted, Capabilities, FakeHttp,
    ProviderRegistry, ProviderState,
};
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const NOW_FUTURE: f64 = 4_000_000_000.0;
/// Text that must never appear in a reply or an event: pieces of the made-up logins.
const SECRETS: [&str; 3] = ["REFRESH-SECRET", "eyJhbGciOiJSUzI1NiJ9", "Bearer"];

fn b64url(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk.iter().fold(0u32, |a, b| (a << 8) | *b as u32) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

fn jwt(claims: &Value) -> String {
    format!(
        "eyJhbGciOiJSUzI1NiJ9.{}.sig",
        b64url(claims.to_string().as_bytes())
    )
}

/// A made-up Codex `auth.json`.
fn codex_login(email: &str, user: &str, ws: &str) -> Vec<u8> {
    let id = jwt(&json!({"email": email, "sub": "x",
        "https://api.openai.com/auth": {"chatgpt_account_id": ws, "chatgpt_user_id": user}}));
    let access = jwt(&json!({"exp": NOW_FUTURE}));
    json!({"tokens": {"id_token": id, "access_token": access,
        "refresh_token": "REFRESH-SECRET", "account_id": ws}})
    .to_string()
    .into_bytes()
}

struct Rig {
    root: PathBuf,
    keychain: Arc<FakeKeychain>,
    http: Arc<FakeHttp>,
    procs: Arc<FakeProcs>,
    apps: Arc<FakeApps>,
    room: Arc<FakeRoom>,
    flag: Arc<SwitchFlag>,
    ops: ProvidersOps,
}

impl Rig {
    fn new(tag: &str, on: bool, procs: FakeProcs, apps: FakeApps) -> Rig {
        let root = std::env::temp_dir().join(format!("nl-provops-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("codex")).unwrap();
        let keychain = Arc::new(FakeKeychain::new());
        let (http, procs, apps) = (Arc::new(FakeHttp::new()), Arc::new(procs), Arc::new(apps));
        let room = Arc::new(FakeRoom::default());
        let flag = SwitchFlag::in_memory(ACCOUNTS, ACCOUNTS_OFF, on);
        let clock: Arc<dyn Fn() -> f64 + Send + Sync> = Arc::new(|| 1_000_000.0);
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(DesktopStore::new(
            DesktopDeps {
                apps: apps.clone(),
                claude_accounts: Arc::new(Vec::new),
            },
            DesktopPaths {
                claude_dir: root.join("Claude"),
                store_dir: root.join("state/desktop"),
                apps_dir: root.join("Applications"),
            },
        )));
        registry.register(Arc::new(CodexStore::new(
            CodexDeps {
                keychain: keychain.clone(),
                http: http.clone(),
                procs: procs.clone(),
                apps: apps.clone(),
                clock: clock.clone(),
            },
            CodexPaths {
                home: root.join("codex"),
                state_dir: root.join("state"),
            },
        )));
        registry.register(Arc::new(AntigravityStore::new(
            AntigravityDeps {
                keychain: keychain.clone(),
                http: http.clone(),
                procs: procs.clone(),
                apps: apps.clone(),
                clock,
            },
            AntigravityPaths {
                token_file: root.join("gemini/jetski-standalone-oauth-token"),
                apps_dir: root.join("Applications"),
                state_dir: root.join("state"),
            },
        )));
        let ops = ProvidersOps::new(flag.clone(), registry, room.clone());
        ops.set_add_timing(Duration::from_millis(10), Duration::from_secs(30));
        Rig {
            root,
            keychain,
            http,
            procs,
            apps,
            room,
            flag,
            ops,
        }
    }

    fn codex_auth(&self) -> PathBuf {
        self.root.join("codex/auth.json")
    }

    fn call(&self, op: &str, args: Value) -> Result<Value, notchling_local_api::ApiError> {
        self.ops.call(op, &args)
    }

    /// Nothing outside the rig's own temp folder was touched.
    fn assert_untouched(&self) {
        assert!(
            self.keychain.calls().is_empty(),
            "{:?}",
            self.keychain.calls()
        );
        assert_eq!(self.http.calls(), 0);
        assert!(self.procs.calls().is_empty(), "{:?}", self.procs.calls());
        assert!(self.apps.events().is_empty(), "{:?}", self.apps.events());
        assert!(!self.root.join("state").exists(), "nothing written");
    }

    fn events(&self) -> Vec<Value> {
        self.room.named("providers.changed")
    }

    fn wait_for(&self, what: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(found) = self.events().into_iter().find(|e| what(e)) {
                return found;
            }
            assert!(
                Instant::now() < deadline,
                "no such event in {:?}",
                self.events()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn no_secrets(value: &Value) {
    let text = value.to_string();
    for secret in SECRETS {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
}

#[test]
fn every_op_but_the_list_is_forbidden_while_accounts_are_off_and_nothing_runs() {
    let rig = Rig::new(
        "off",
        false,
        FakeProcs::new(),
        FakeApps::new().running("com.openai.codex"),
    );
    std::fs::write(rig.codex_auth(), codex_login("a@example.com", "u1", "w1")).unwrap();
    let before = std::fs::read(rig.codex_auth()).unwrap();
    let cases = [
        (
            ops::PROVIDERS_SWITCH,
            json!({"provider": "codex", "id": "u2:w2"}),
        ),
        (
            ops::PROVIDERS_SWITCH,
            json!({"provider": "codex", "id": "u2:w2", "confirm": true}),
        ),
        (ops::PROVIDERS_ADD_BEGIN, json!({"provider": "antigravity"})),
        (
            ops::PROVIDERS_ADD_CANCEL,
            json!({"provider": "claudeDesktop"}),
        ),
        (
            ops::PROVIDERS_REMOVE,
            json!({"provider": "codex", "id": "u1:w1"}),
        ),
        (
            ops::PROVIDERS_USAGE,
            json!({"provider": "codex", "refresh": true}),
        ),
        (ops::PROVIDERS_USAGE, json!({})),
    ];
    for (op, args) in cases {
        let err = rig.call(op, args).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden, "{op}");
        assert_eq!(err.message, ACCOUNTS_OFF, "{op}");
    }
    assert_eq!(
        ACCOUNTS_OFF,
        "accounts are off: the Swift app owns logins until the maintainer turns this on"
    );
    rig.assert_untouched();
    assert_eq!(std::fs::read(rig.codex_auth()).unwrap(), before);
    assert!(rig.events().is_empty());
}

#[test]
fn the_list_while_off_shows_the_saved_names_and_reads_no_login() {
    let rig = Rig::new("listoff", false, FakeProcs::new(), FakeApps::new());
    // The daemon's own list from an earlier run with accounts on.
    std::fs::create_dir_all(rig.root.join("state")).unwrap();
    std::fs::write(
        rig.root.join("state/codex.json"),
        r#"{"version":1,"accounts":[{"id":"u1:w1","email":"a@example.com","plan":"Plus"}]}"#,
    )
    .unwrap();
    // The providers are made after the file, as the daemon makes them at start.
    let ops2 = {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(CodexStore::new(
            CodexDeps {
                keychain: rig.keychain.clone(),
                http: rig.http.clone(),
                procs: rig.procs.clone(),
                apps: rig.apps.clone(),
                clock: Arc::new(|| 0.0),
            },
            CodexPaths {
                home: rig.root.join("codex"),
                state_dir: rig.root.join("state"),
            },
        )));
        ProvidersOps::new(rig.flag.clone(), registry, rig.room.clone())
    };
    let reply = ops2.call(ops::PROVIDERS_LIST, &json!({})).unwrap();
    assert_eq!(reply["accounts_on"], false);
    let codex = &reply["providers"][0];
    assert_eq!(codex["id"], "codex");
    assert_eq!(codex["state"]["accounts"][0]["email"], "a@example.com");
    assert_eq!(codex["state"]["active_id"], Value::Null);
    assert_eq!(
        codex["available"],
        Value::Null,
        "not even whether an app is installed"
    );
    assert!(rig.keychain.calls().is_empty() && rig.http.calls() == 0);
    assert!(rig.procs.calls().is_empty() && rig.apps.events().is_empty());
}

#[test]
fn the_list_while_on_reads_the_live_logins_and_says_who_is_active() {
    let rig = Rig::new("liston", true, FakeProcs::new(), FakeApps::new());
    std::fs::write(rig.codex_auth(), codex_login("a@example.com", "u1", "w1")).unwrap();
    let reply = rig.call(ops::PROVIDERS_LIST, json!({})).unwrap();
    assert_eq!(reply["accounts_on"], true);
    let ids: Vec<_> = reply["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["claudeDesktop", "codex", "antigravity"]);
    let codex = &reply["providers"][1];
    assert_eq!(codex["state"]["active_id"], "u1:w1");
    assert_eq!(codex["available"], true);
    assert_eq!(
        codex["capabilities"],
        json!(["addAccount", "usage", "autoSwitch"])
    );
    assert!(reply.get("syncing").is_none());
    no_secrets(&reply);
    assert!(
        rig.keychain.get("Notchling Codex", "u1:w1").is_some(),
        "the live login was saved"
    );
    // sync: false reads nothing.
    let before = rig.keychain.calls().len();
    rig.call(ops::PROVIDERS_LIST, json!({"sync": false}))
        .unwrap();
    assert_eq!(rig.keychain.calls().len(), before);
}

#[test]
fn a_switch_that_would_interrupt_work_asks_first_and_runs_only_when_confirmed() {
    let rig = Rig::new(
        "confirm",
        true,
        FakeProcs::new(),
        FakeApps::new().running("com.openai.codex"),
    );
    std::fs::write(rig.codex_auth(), codex_login("a@example.com", "u1", "w1")).unwrap();
    rig.keychain.write("Notchling Codex", "u2:w2", "x");
    let asked = rig
        .call(
            ops::PROVIDERS_SWITCH,
            json!({"provider": "codex", "id": "u2:w2"}),
        )
        .unwrap();
    assert_eq!(asked["state"], "needs_confirmation");
    assert_eq!(asked["confirmation"]["title"], "Switch Codex Account?");
    assert!(
        !rig.apps.events().iter().any(|e| e.starts_with("quit")),
        "nothing quit before the answer"
    );
    // Confirmed, but the saved copy isn't a login: the switch fails cleanly, in fixed words.
    let err = rig
        .call(
            ops::PROVIDERS_SWITCH,
            json!({"provider": "codex", "id": "u2:w2", "confirm": true}),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    let failed = rig.wait_for(|e| e["reason"] == "switch" && e["state"] == "failed");
    assert_eq!(failed["title"], "Couldn't switch Codex accounts");
    no_secrets(&failed);
}

#[test]
fn a_confirmed_switch_replies_and_announces() {
    let rig = Rig::new("switch", true, FakeProcs::new(), FakeApps::new());
    let a = codex_login("a@example.com", "u1", "w1");
    let b = codex_login("b@example.com", "u2", "w2");
    std::fs::write(rig.codex_auth(), &a).unwrap();
    {
        use base64_for_tests::encode;
        rig.keychain.write("Notchling Codex", "u2:w2", &encode(&b));
    }
    let reply = rig
        .call(
            ops::PROVIDERS_SWITCH,
            json!({"provider": "codex", "id": "u2:w2"}),
        )
        .unwrap();
    assert_eq!(
        reply,
        json!({"state": "switched", "provider": "codex", "id": "u2:w2"})
    );
    assert_eq!(std::fs::read(rig.codex_auth()).unwrap(), b);
    let done = rig.wait_for(|e| e["reason"] == "switch" && e["state"] == "done");
    assert_eq!(done["id"], "u2:w2");
    no_secrets(&done);
    let state = rig
        .call(ops::PROVIDERS_LIST, json!({"sync": false}))
        .unwrap();
    assert_eq!(state["providers"][1]["state"]["active_id"], "u2:w2");
}

mod base64_for_tests {
    /// Standard base64 with padding, for the saved copy the store reads back.
    pub fn encode(data: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let n = chunk.iter().fold(0u32, |a, b| (a << 8) | *b as u32) << (8 * (3 - chunk.len()));
            for i in 0..=chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            }
            for _ in chunk.len()..3 {
                out.push('=');
            }
        }
        out
    }
}

/// A provider whose switch takes a while, to see the reply give up and the event follow.
struct Slow {
    done: Arc<Mutex<bool>>,
}

impl AccountProvider for Slow {
    fn id(&self) -> notchling_engine::providers::ProviderId {
        "slow".into()
    }
    fn title(&self) -> &'static str {
        "Slow"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::ADD_ACCOUNT
    }
    fn state(&self) -> ProviderState {
        ProviderState::default()
    }
    fn sync(&self, _force: bool) {}
    fn switch_to(&self, _id: &str) -> Result<(), AccountError> {
        std::thread::sleep(Duration::from_millis(300));
        *self.done.lock().unwrap() = true;
        Ok(())
    }
    fn begin_add(&self) -> Result<AddStarted, AccountError> {
        std::thread::sleep(Duration::from_millis(300));
        Ok(AddStarted::default())
    }
    fn poll_add(&self) -> AddProgress {
        AddProgress::Done
    }
    fn cancel_add(&self) {}
    fn remove(&self, _id: &str) {}
    fn switch_failure_title(&self) -> &'static str {
        "Couldn't switch Slow"
    }
}

fn slow_rig() -> (ProvidersOps, Arc<FakeRoom>, Arc<Mutex<bool>>) {
    let done = Arc::new(Mutex::new(false));
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(Slow { done: done.clone() }));
    let room = Arc::new(FakeRoom::default());
    let ops = ProvidersOps::new(
        SwitchFlag::in_memory(ACCOUNTS, ACCOUNTS_OFF, true),
        registry,
        room.clone(),
    );
    ops.set_reply_wait(Duration::from_millis(30));
    ops.set_add_timing(Duration::from_millis(10), Duration::from_secs(5));
    (ops, room, done)
}

fn wait_event(room: &FakeRoom, what: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(e) = room
            .named("providers.changed")
            .into_iter()
            .find(|e| what(e))
        {
            return e;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_slow_switch_answers_switching_and_ends_as_an_event_and_one_runs_at_a_time() {
    let (ops, room, done) = slow_rig();
    let args = json!({"provider": "slow", "id": "x"});
    assert_eq!(
        ops.call(ops::PROVIDERS_SWITCH, &args).unwrap(),
        json!({"state": "switching"})
    );
    let second = ops.call(ops::PROVIDERS_SWITCH, &args).unwrap_err();
    assert_eq!(second.code, ErrorCode::Failed);
    assert!(second.message.contains("another account change"));
    let end = wait_event(&room, |e| e["reason"] == "switch" && e["state"] == "done");
    assert_eq!(end["provider"], "slow");
    assert!(*done.lock().unwrap());
    // The slot is free again.
    let again = ops.call(ops::PROVIDERS_SWITCH, &args).unwrap();
    assert!(again["state"] == "switching" || again["state"] == "switched");
}

#[test]
fn a_slow_add_start_answers_starting_and_reports_progress_by_event() {
    let (ops, room, _) = slow_rig();
    let reply = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "slow"}))
        .unwrap();
    assert_eq!(reply, json!({"state": "starting"}));
    wait_event(&room, |e| e["reason"] == "add" && e["state"] == "started");
    wait_event(&room, |e| e["reason"] == "add" && e["state"] == "done");
}

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
    std::fs::write(
        scratch.join("auth.json"),
        codex_login("c@example.com", "u3", "w3"),
    )
    .unwrap();
    rig.procs.login_state().lock().unwrap().exit = Some(0);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        rig.keychain.calls().len(),
        calls,
        "no Keychain call after the switch went off"
    );
    assert!(!rig.events().iter().any(|e| e["state"] == "done"));
    // And a cancel is refused like any op.
    assert_eq!(
        rig.call(ops::PROVIDERS_ADD_CANCEL, json!({"provider": "codex"}))
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
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
    assert!(!off.procs.login_state().lock().unwrap().killed);
}

#[test]
fn usage_reports_the_limits_the_provider_read_and_never_a_token() {
    let rig = Rig::new("usage", true, FakeProcs::new(), FakeApps::new());
    std::fs::write(rig.codex_auth(), codex_login("a@example.com", "u1", "w1")).unwrap();
    rig.http.ok(200, r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":42,"limit_window_seconds":18000,"reset_at":1800000000}}}"#);
    let reply = rig
        .call(
            ops::PROVIDERS_USAGE,
            json!({"provider": "codex", "refresh": true}),
        )
        .unwrap();
    let report = &reply["reports"]["u1:w1"];
    assert_eq!(report["groups"][0]["name"], "Codex");
    assert_eq!(
        report["groups"][0]["bars"][0],
        json!({"label": "5h", "kind": "session", "percent": 42.0, "resets_at": 1800000000.0})
    );
    no_secrets(&reply);
    assert_eq!(rig.http.calls(), 1);
    let req = &rig.http.requests()[0];
    assert!(!format!("{req:?}").contains("Bearer"));
}

#[test]
fn bad_arguments_are_bad_requests() {
    let rig = Rig::new("bad", true, FakeProcs::new(), FakeApps::new());
    for (op, args) in [
        (ops::PROVIDERS_SWITCH, json!({})),
        (
            ops::PROVIDERS_SWITCH,
            json!({"provider": "nope", "id": "x"}),
        ),
        (ops::PROVIDERS_SWITCH, json!({"provider": "codex"})),
        (
            ops::PROVIDERS_SWITCH,
            json!({"provider": "codex", "id": ""}),
        ),
        (
            ops::PROVIDERS_SWITCH,
            json!({"provider": "codex", "id": "a\nb"}),
        ),
        (
            ops::PROVIDERS_SWITCH,
            json!({"provider": "codex", "id": "x".repeat(201)}),
        ),
        (ops::PROVIDERS_REMOVE, json!({"provider": "codex", "id": 4})),
        (ops::PROVIDERS_USAGE, json!({"provider": "windsurf"})),
    ] {
        assert_eq!(
            rig.call(op, args.clone()).unwrap_err().code,
            ErrorCode::BadRequest,
            "{op} {args}"
        );
    }
}

#[test]
fn removing_forgets_the_saved_login() {
    let rig = Rig::new("remove", true, FakeProcs::new(), FakeApps::new());
    rig.keychain.write("Notchling Codex", "u2:w2", "x");
    let reply = rig
        .call(
            ops::PROVIDERS_REMOVE,
            json!({"provider": "codex", "id": "u2:w2"}),
        )
        .unwrap();
    assert_eq!(reply, json!({"removed": true}));
    assert!(rig.keychain.get("Notchling Codex", "u2:w2").is_none());
    rig.wait_for(|e| e["reason"] == "remove");
}

#[test]
fn every_op_is_app_tier_and_nothing_else_is_ours() {
    let rig = Rig::new("spec", false, FakeProcs::new(), FakeApps::new());
    for op in PROVIDERS_OPS {
        assert_eq!(
            rig.ops.spec(op),
            Some(notchling_local_api::OpSpec::APP),
            "{op}"
        );
    }
    assert_eq!(rig.ops.spec(ops::SESSIONS_LIST), None);
    assert_eq!(rig.ops.spec("providers.tokens"), None);
}

#[test]
fn a_daemon_over_a_fixture_root_uses_fakes_and_lists_in_the_windows_order() {
    let root = std::env::temp_dir().join(format!("nl-provops-fixture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
    let ops = ProvidersOps::for_daemon(
        &layout,
        SwitchFlag::off(ACCOUNTS, ACCOUNTS_OFF),
        Arc::new(FakeRoom::default()),
    );
    let reply = ops.call(ops::PROVIDERS_LIST, &json!({})).unwrap();
    let ids: Vec<_> = reply["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["claudeDesktop", "codex", "antigravity"]);
    assert!(!root.exists(), "building and listing wrote nothing");
}
