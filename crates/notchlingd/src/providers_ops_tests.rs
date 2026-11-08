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
use notchling_engine::accounts::procs::{FakeApps, FakeProcs};
use notchling_engine::providers::{FakeHttp, ProviderRegistry};
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
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
pub(crate) fn codex_login(email: &str, user: &str, ws: &str) -> Vec<u8> {
    let id = jwt(&json!({"email": email, "sub": "x",
        "https://api.openai.com/auth": {"chatgpt_account_id": ws, "chatgpt_user_id": user}}));
    let access = jwt(&json!({"exp": NOW_FUTURE}));
    json!({"tokens": {"id_token": id, "access_token": access,
        "refresh_token": "REFRESH-SECRET", "account_id": ws}})
    .to_string()
    .into_bytes()
}

pub(crate) struct Rig {
    pub(crate) root: PathBuf,
    pub(crate) keychain: Arc<FakeKeychain>,
    pub(crate) http: Arc<FakeHttp>,
    pub(crate) procs: Arc<FakeProcs>,
    pub(crate) apps: Arc<FakeApps>,
    pub(crate) room: Arc<FakeRoom>,
    pub(crate) flag: Arc<SwitchFlag>,
    pub(crate) ops: ProvidersOps,
}

impl Rig {
    pub(crate) fn new(tag: &str, on: bool, procs: FakeProcs, apps: FakeApps) -> Rig {
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

    pub(crate) fn codex_auth(&self) -> PathBuf {
        self.root.join("codex/auth.json")
    }

    pub(crate) fn call(
        &self,
        op: &str,
        args: Value,
    ) -> Result<Value, notchling_local_api::ApiError> {
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

    pub(crate) fn events(&self) -> Vec<Value> {
        self.room.named("providers.changed")
    }

    pub(crate) fn wait_for(&self, what: impl Fn(&Value) -> bool) -> Value {
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

pub(crate) fn no_secrets(value: &Value) {
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
    rig.keychain.write("Notchling Codex", "u2:w2", "x").unwrap();
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
        rig.keychain
            .write("Notchling Codex", "u2:w2", &encode(&b))
            .unwrap();
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
    rig.keychain.write("Notchling Codex", "u2:w2", "x").unwrap();
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
