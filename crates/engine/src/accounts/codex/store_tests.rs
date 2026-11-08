//! The Codex store against fakes: a temp `CODEX_HOME`, an in-memory Keychain, no network, no
//! process, no real app.

use super::login;
use super::store::{CodexDeps, CodexPaths, CodexStore, APP_BUNDLE_ID, SERVICE};
use super::testkit::{login as login_at, login_for};
use crate::accounts::generic_password::{FakeKeychain, Keychain};
use crate::accounts::procs::{FakeApps, FakeProcs, ProcCall};
use crate::providers::{AccountProvider, AddProgress, FakeHttp, Trust};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::path::PathBuf;
use std::sync::Arc;

const NOW: f64 = 1_000_000.0;
const FUTURE: f64 = NOW + 86_400.0;

struct Rig {
    root: PathBuf,
    keychain: Arc<FakeKeychain>,
    http: Arc<FakeHttp>,
    procs: Arc<FakeProcs>,
    apps: Arc<FakeApps>,
    store: CodexStore,
}

impl Rig {
    fn new(tag: &str, procs: FakeProcs, apps: FakeApps) -> Rig {
        let root = std::env::temp_dir().join(format!("nl-codex-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("codex")).unwrap();
        Rig::at(root, Arc::new(FakeKeychain::new()), procs, apps)
    }

    fn at(root: PathBuf, keychain: Arc<FakeKeychain>, procs: FakeProcs, apps: FakeApps) -> Rig {
        let (http, procs, apps) = (Arc::new(FakeHttp::new()), Arc::new(procs), Arc::new(apps));
        let store = CodexStore::new(
            CodexDeps {
                keychain: keychain.clone(),
                http: http.clone(),
                procs: procs.clone(),
                apps: apps.clone(),
                clock: Arc::new(|| NOW),
            },
            CodexPaths {
                home: root.join("codex"),
                state_dir: root.join("state"),
            },
        );
        Rig {
            root,
            keychain,
            http,
            procs,
            apps,
            store,
        }
    }

    fn auth(&self) -> PathBuf {
        self.root.join("codex/auth.json")
    }

    fn live(&self, login: &[u8]) {
        std::fs::write(self.auth(), login).unwrap();
    }

    fn saved(&self, id: &str) -> Option<Vec<u8>> {
        self.keychain
            .get(SERVICE, id)
            .and_then(|v| STANDARD.decode(v).ok())
    }

    fn save(&self, id: &str, login: &[u8]) {
        assert_eq!(
            self.keychain.write(SERVICE, id, &STANDARD.encode(login)),
            Ok(())
        );
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn two_accounts(rig: &Rig) -> (Vec<u8>, Vec<u8>) {
    let a = login_for("a@example.com", "user-1", "ws-1", FUTURE, "rt-a");
    let b = login_for("b@example.com", "user-2", "ws-2", FUTURE, "rt-b");
    rig.save("user-2:ws-2", &b);
    rig.live(&a);
    (a, b)
}

#[test]
fn a_switch_saves_the_live_login_first_then_writes_the_other_and_restarts_the_server() {
    let procs = FakeProcs::new()
        .answer(
            "/bin/ps -axo command=",
            "/opt/homebrew/bin/codex app-server --managed-daemon\n",
        )
        .binary("codex", &PathBuf::from("/opt/homebrew/bin/codex"));
    let rig = Rig::new("switch", procs, FakeApps::new().running(APP_BUNDLE_ID));
    let (a, b) = two_accounts(&rig);
    // Codex renewed the live login since Notchling last saw it: the saved copy must not be older.
    let renewed = login_for("a@example.com", "user-1", "ws-1", FUTURE, "rt-a-renewed");
    rig.live(&renewed);
    let seen = Arc::new(std::sync::Mutex::new(None));
    let (auth, seen2) = (rig.auth(), seen.clone());
    rig.apps
        .when_quitting(move |_| *seen2.lock().unwrap() = std::fs::read(&auth).ok());

    rig.store.switch_to("user-2:ws-2").unwrap();

    assert_eq!(
        seen.lock().unwrap().as_deref(),
        Some(renewed.as_slice()),
        "the app quit before auth.json was touched"
    );
    assert_eq!(std::fs::read(rig.auth()).unwrap(), b);
    assert_eq!(
        rig.saved("user-1:ws-1").unwrap(),
        renewed,
        "the renewed login is kept"
    );
    assert_ne!(renewed, a);
    assert_eq!(
        rig.apps
            .events()
            .iter()
            .filter(|e| !e.starts_with("is_running"))
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            format!("quit {APP_BUNDLE_ID}"),
            format!("open {APP_BUNDLE_ID}")
        ]
    );
    assert!(rig.procs.calls().contains(&ProcCall::RunNull(
        "/opt/homebrew/bin/codex app-server daemon restart".into()
    )));
    let state = rig.store.state();
    assert_eq!(state.active_id.as_deref(), Some("user-2:ws-2"));
    assert!(!state.busy);
}

#[test]
fn the_server_is_not_restarted_when_it_is_not_running() {
    let procs = FakeProcs::new()
        .answer("/bin/ps -axo command=", "/usr/bin/vim\n")
        .binary("codex", &PathBuf::from("/opt/homebrew/bin/codex"));
    let rig = Rig::new("noserver", procs, FakeApps::new());
    two_accounts(&rig);
    rig.store.switch_to("user-2:ws-2").unwrap();
    assert!(!rig
        .procs
        .calls()
        .iter()
        .any(|c| matches!(c, ProcCall::RunNull(_))));
    assert!(
        !rig.apps.events().iter().any(|e| e.starts_with("open")),
        "it was not running"
    );
}

#[test]
fn an_app_that_will_not_quit_leaves_the_login_alone() {
    let rig = Rig::new(
        "stubborn",
        FakeProcs::new(),
        FakeApps::new()
            .running(APP_BUNDLE_ID)
            .stubborn(APP_BUNDLE_ID),
    );
    let (a, _) = two_accounts(&rig);
    let err = rig.store.switch_to("user-2:ws-2").unwrap_err();
    assert!(err.message().contains("didn't quit"));
    assert_eq!(std::fs::read(rig.auth()).unwrap(), a);
    assert!(!rig.store.state().busy);
    assert!(!rig
        .procs
        .calls()
        .iter()
        .any(|c| matches!(c, ProcCall::RunNull(_))));
}

#[test]
fn a_switch_to_an_account_with_no_saved_login_quits_nothing() {
    let rig = Rig::new(
        "nosaved",
        FakeProcs::new(),
        FakeApps::new().running(APP_BUNDLE_ID),
    );
    let (a, _) = two_accounts(&rig);
    let err = rig.store.switch_to("nobody:ws-9").unwrap_err();
    assert!(err.message().contains("no saved login"));
    assert_eq!(std::fs::read(rig.auth()).unwrap(), a);
    assert!(!rig.apps.events().iter().any(|e| e.starts_with("quit")));
}

#[test]
fn sync_lists_the_live_account_and_remembers_it_across_a_restart() {
    let rig = Rig::new("sync", FakeProcs::new(), FakeApps::new());
    rig.live(&login_at(FUTURE));
    rig.http.ok(200, r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":30,"limit_window_seconds":18000,"reset_at":1800000000}}}"#);
    rig.store.sync(false);
    let state = rig.store.state();
    assert_eq!(state.active_id.as_deref(), Some("user-1:ws-1"));
    assert_eq!(state.accounts[0].email, "a@example.com");
    assert_eq!(
        state.accounts[0].plan.as_deref(),
        Some("Pro"),
        "the usage endpoint's plan wins over the id token's"
    );
    assert!(
        rig.saved("user-1:ws-1").is_some(),
        "the live login is saved"
    );
    let report = rig.store.usage_report("user-1:ws-1").unwrap();
    assert_eq!(report.groups[0].name.as_deref(), Some("Codex"));
    assert_eq!(report.bars()[0].percent, 30.0);
    // The daemon's own list survives a restart; nothing is read from the Keychain or Codex to show it.
    let kc = rig.keychain.clone();
    let again = Rig::at(rig.root.clone(), kc, FakeProcs::new(), FakeApps::new());
    let names: Vec<_> = again
        .store
        .state()
        .accounts
        .iter()
        .map(|a| a.email.clone())
        .collect();
    assert_eq!(names, vec!["a@example.com"]);
    assert_eq!(again.keychain.calls().len(), rig.keychain.calls().len());
}

#[test]
fn limits_are_asked_for_with_the_workspace_and_cached_for_two_minutes() {
    let rig = Rig::new("limits", FakeProcs::new(), FakeApps::new());
    rig.live(&login_at(FUTURE));
    rig.http.ok(
        200,
        r#"{"rate_limit":{"primary_window":{"used_percent":1,"limit_window_seconds":18000}}}"#,
    );
    rig.store.sync(false);
    rig.store.sync(false);
    assert_eq!(
        rig.http.calls(),
        1,
        "the second read is inside the two minutes"
    );
    rig.http.ok(200, "{}");
    rig.store.sync(true);
    assert_eq!(rig.http.calls(), 2, "force reads again");
    let req = &rig.http.requests()[0];
    assert_eq!(req.url, "https://chatgpt.com/backend-api/wham/usage");
    assert_eq!(req.method, "GET");
    assert_eq!(req.trust, Trust::System);
    let header = |n: &str| {
        req.headers
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(header("ChatGPT-Account-Id").as_deref(), Some("ws-1"));
    assert!(header("Authorization").unwrap().starts_with("Bearer "));
    assert!(!format!("{req:?}").contains("Bearer"));
}

#[test]
fn an_expired_token_is_never_renewed_so_no_request_is_made() {
    let rig = Rig::new("expired", FakeProcs::new(), FakeApps::new());
    rig.live(&login_at(NOW - 5.0));
    rig.store.sync(true);
    assert_eq!(rig.http.calls(), 0);
    assert!(rig.store.usage_report("user-1:ws-1").is_none());
    assert_eq!(rig.store.state().active_id.as_deref(), Some("user-1:ws-1"));
}

#[test]
fn a_saved_account_that_is_not_live_is_read_from_its_saved_copy() {
    let rig = Rig::new("saved-usage", FakeProcs::new(), FakeApps::new());
    two_accounts(&rig);
    rig.store.sync(false); // learns account 1 from the live file
    let mut book = crate::accounts::book::read_accounts(&rig.root.join("state/codex.json"));
    book.push(crate::accounts::book::Saved {
        id: "user-2:ws-2".into(),
        email: "b@example.com".into(),
        plan: None,
    });
    crate::accounts::book::write_accounts(&rig.root.join("state/codex.json"), &book);
    let rig2 = Rig::at(
        rig.root.clone(),
        rig.keychain.clone(),
        FakeProcs::new(),
        FakeApps::new(),
    );
    rig2.http.ok(200, "{}");
    rig2.http.ok(200, "{}");
    rig2.store.sync(true);
    let workspaces: Vec<_> = rig2
        .http
        .requests()
        .iter()
        .filter_map(|r| {
            r.headers
                .iter()
                .find(|(k, _)| k == "ChatGPT-Account-Id")
                .map(|(_, v)| v.clone())
        })
        .collect();
    assert_eq!(workspaces, vec!["ws-1", "ws-2"]);
}

#[test]
fn adding_runs_codex_login_in_a_scratch_home_and_leaves_the_live_login_alone() {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let rig = Rig::new("add", procs, FakeApps::new());
    let (a, _) = two_accounts(&rig);
    rig.store.begin_add().unwrap();
    let spawn = rig
        .procs
        .calls()
        .into_iter()
        .find_map(|c| match c {
            ProcCall::SpawnLogin(s) => Some(s),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        spawn.args,
        vec!["login", "-c", "cli_auth_credentials_store=\"file\""]
    );
    let scratch = PathBuf::from(&spawn.env[0].1);
    assert_eq!(spawn.env[0].0, "CODEX_HOME");
    assert_ne!(scratch, rig.root.join("codex"));
    assert!(scratch.starts_with(rig.root.join("state/codex-login")));
    assert!(rig.store.state().adding);

    let script = rig.procs.login_state();
    script.lock().unwrap().output = "Starting local login server...\nOpen https://auth.openai.com/oauth/authorize?x=1 to sign in\n".into();
    assert_eq!(
        rig.store.poll_add(),
        AddProgress::Waiting {
            sign_in_url: Some("https://auth.openai.com/oauth/authorize?x=1".into())
        }
    );
    assert_eq!(
        rig.store.state().sign_in_url.as_deref(),
        Some("https://auth.openai.com/oauth/authorize?x=1")
    );

    let c = login_for("c@example.com", "user-3", "ws-3", FUTURE, "rt-c");
    std::fs::write(scratch.join("auth.json"), &c).unwrap();
    script.lock().unwrap().exit = Some(0);
    rig.http.ok(200, "{}");
    rig.http.ok(200, "{}");
    rig.http.ok(200, "{}");
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(rig.saved("user-3:ws-3").unwrap(), c);
    assert!(!scratch.exists(), "the scratch home is gone");
    assert_eq!(
        std::fs::read(rig.auth()).unwrap(),
        a,
        "the live login is untouched"
    );
    assert!(rig
        .store
        .state()
        .accounts
        .iter()
        .any(|x| x.email == "c@example.com"));
    assert!(!rig.store.state().adding);
    assert_eq!(rig.store.poll_add(), AddProgress::Idle);
}

#[test]
fn a_cancelled_add_kills_codex_and_saves_nothing() {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let rig = Rig::new("cancel", procs, FakeApps::new());
    rig.live(&login_at(FUTURE));
    rig.store.begin_add().unwrap();
    let writes_before = rig.keychain.calls().len();
    rig.store.cancel_add();
    assert!(rig.procs.login_state().lock().unwrap().killed);
    assert_eq!(rig.store.poll_add(), AddProgress::Cancelled);
    assert_eq!(rig.keychain.calls().len(), writes_before);
    assert!(!rig.store.state().adding);
    let left: Vec<_> = std::fs::read_dir(rig.root.join("state/codex-login"))
        .unwrap()
        .collect();
    assert!(left.is_empty());
}

#[test]
fn a_sign_in_that_fails_says_so_without_saying_why_in_detail() {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let rig = Rig::new("failed", procs, FakeApps::new());
    rig.store.begin_add().unwrap();
    rig.procs.login_state().lock().unwrap().exit = Some(1);
    match rig.store.poll_add() {
        AddProgress::Failed(e) => assert_eq!(e.message(), "Codex didn't finish signing in."),
        other => panic!("{other:?}"),
    }
    assert!(rig.store.state().accounts.is_empty());
}

#[test]
fn adding_without_codex_installed_says_so_and_starts_nothing() {
    let rig = Rig::new("nocodex", FakeProcs::new(), FakeApps::new());
    let err = rig.store.begin_add().unwrap_err();
    assert!(err.message().contains("Couldn't find the codex command"));
    assert!(!rig
        .procs
        .calls()
        .iter()
        .any(|c| matches!(c, ProcCall::SpawnLogin(_))));
}

#[test]
fn removing_forgets_the_saved_login_and_the_name() {
    let rig = Rig::new("remove", FakeProcs::new(), FakeApps::new());
    two_accounts(&rig);
    rig.store.sync(false);
    rig.store.remove("user-2:ws-2");
    assert!(rig.saved("user-2:ws-2").is_none());
    assert!(rig
        .store
        .state()
        .accounts
        .iter()
        .all(|a| a.id != "user-2:ws-2"));
    assert!(
        rig.saved("user-1:ws-1").is_some(),
        "the live account's copy stays"
    );
}

#[test]
fn what_a_switch_would_interrupt_is_read_from_ps_and_the_app() {
    let procs = FakeProcs::new().answer(
        "/bin/ps -axo command=",
        "/usr/local/bin/codex exec --json\n/usr/local/bin/codex app-server --managed-daemon\n",
    );
    let rig = Rig::new("work", procs, FakeApps::new().running(APP_BUNDLE_ID));
    let work = rig.store.running_work();
    assert_eq!((work.app, work.server, work.sessions), (true, true, 1));
    assert!(!login::idle_for_auto_switch(&work));
    let confirm = rig.store.switch_confirmation("x").unwrap();
    assert_eq!(confirm.title, "Switch Codex Account?");
    assert!(confirm.message.contains("ChatGPT app quits"));
}

#[test]
fn the_sign_in_page_is_found_among_colour_codes_and_nothing_else_is() {
    use super::store::sign_in_url;
    assert_eq!(
        sign_in_url("go to \u{1b}[4mhttps://auth.openai.com/oauth/authorize?a=1&b=2\u{1b}[0m now")
            .as_deref(),
        Some("https://auth.openai.com/oauth/authorize?a=1&b=2")
    );
    assert_eq!(
        sign_in_url("https://auth.openai.com/"),
        None,
        "no path, no page"
    );
    assert_eq!(
        sign_in_url("https://evil.example/https://auth.openai.com/x").as_deref(),
        Some("https://auth.openai.com/x")
    );
    assert_eq!(sign_in_url("open https://example.com/login"), None);
}
