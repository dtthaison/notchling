//! The Antigravity store against fakes: a temp token file, an in-memory Keychain, no process, no
//! network, no real app.

use super::login::{decode_keyring, encode_keyring, fake_email_login, fake_login};
use super::store::{
    AntigravityDeps, AntigravityPaths, AntigravityStore, BUNDLE_IDS, LIVE_ACCOUNT, LIVE_SERVICE,
    SERVICE,
};
use crate::accounts::generic_password::{FakeKeychain, Keychain};
use crate::accounts::procs::{FakeApps, FakeProcs};
use crate::providers::{AccountProvider, AddProgress, FakeHttp};
use std::path::PathBuf;
use std::sync::Arc;

struct Rig {
    root: PathBuf,
    keychain: Arc<FakeKeychain>,
    http: Arc<FakeHttp>,
    apps: Arc<FakeApps>,
    store: AntigravityStore,
}

impl Rig {
    fn new(tag: &str, apps: FakeApps) -> Rig {
        Rig::with_procs(tag, FakeProcs::new(), apps)
    }

    fn with_procs(tag: &str, procs: FakeProcs, apps: FakeApps) -> Rig {
        let root = std::env::temp_dir().join(format!("nl-agy-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("gemini")).unwrap();
        let keychain = Arc::new(FakeKeychain::new());
        let (http, apps) = (Arc::new(FakeHttp::new()), Arc::new(apps));
        let store = AntigravityStore::new(
            AntigravityDeps {
                keychain: keychain.clone(),
                http: http.clone(),
                procs: Arc::new(procs),
                apps: apps.clone(),
                clock: Arc::new(|| 100.0),
            },
            AntigravityPaths {
                token_file: root.join("gemini/jetski-standalone-oauth-token"),
                apps_dir: root.join("Applications"),
                state_dir: root.join("state"),
            },
        );
        Rig {
            root,
            keychain,
            http,
            apps,
            store,
        }
    }

    fn token_file(&self) -> PathBuf {
        self.root.join("gemini/jetski-standalone-oauth-token")
    }

    /// Antigravity's own item, as it would have written it.
    fn set_live(&self, login: &str) {
        self.keychain
            .write(LIVE_SERVICE, LIVE_ACCOUNT, &encode_keyring(login))
            .unwrap();
    }

    fn live(&self) -> Option<String> {
        self.keychain
            .get(LIVE_SERVICE, LIVE_ACCOUNT)
            .and_then(|v| decode_keyring(&v))
    }

    fn save(&self, id: &str, login: &str) {
        assert_eq!(self.keychain.write(SERVICE, id, login), Ok(()));
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn sync_saves_the_live_login_under_its_account() {
    let rig = Rig::new("sync", FakeApps::new());
    let login = fake_login("1234", "a@example.com", "t1");
    rig.set_live(&login);
    rig.store.sync(false);
    assert_eq!(
        rig.keychain.get(SERVICE, "1234").as_deref(),
        Some(login.as_str())
    );
    let state = rig.store.state();
    assert_eq!(state.active_id.as_deref(), Some("1234"));
    assert_eq!(state.accounts[0].email, "a@example.com");
    // A second sync of an unchanged login doesn't write the Keychain again.
    let writes = rig
        .keychain
        .calls()
        .iter()
        .filter(|c| c.starts_with("write"))
        .count();
    rig.store.sync(false);
    assert_eq!(
        rig.keychain
            .calls()
            .iter()
            .filter(|c| c.starts_with("write"))
            .count(),
        writes
    );
}

#[test]
fn the_email_only_format_joins_the_saved_account_with_that_email() {
    let rig = Rig::new("email", FakeApps::new());
    rig.set_live(&fake_login("1234", "A@example.com", "t1"));
    rig.store.sync(false);
    rig.set_live(&fake_email_login("a@EXAMPLE.com", "t2"));
    rig.store.sync(false);
    let state = rig.store.state();
    assert_eq!(state.accounts.len(), 1, "no second row for the same person");
    assert_eq!(state.active_id.as_deref(), Some("1234"));
    // An email Notchling hasn't seen is its own account until an id login names it.
    rig.set_live(&fake_email_login("new@example.com", "t3"));
    rig.store.sync(false);
    assert_eq!(
        rig.store.state().active_id.as_deref(),
        Some("email:new@example.com")
    );
}

#[test]
fn the_file_mirror_is_read_when_the_item_is_missing() {
    let rig = Rig::new("mirror", FakeApps::new());
    std::fs::write(rig.token_file(), fake_login("77", "m@example.com", "t")).unwrap();
    rig.store.sync(false);
    assert_eq!(rig.store.state().active_id.as_deref(), Some("77"));
}

#[test]
fn a_switch_quits_first_then_sets_the_item_and_the_mirror_then_reopens() {
    let rig = Rig::new(
        "switch",
        FakeApps::new()
            .running(BUNDLE_IDS[0])
            .running(BUNDLE_IDS[1]),
    );
    let a = fake_login("1", "a@example.com", "ta");
    let b = fake_login("2", "b@example.com", "tb");
    rig.set_live(&a);
    rig.save("2", &b);
    let seen = Arc::new(std::sync::Mutex::new(vec![]));
    let (kc, s2) = (rig.keychain.clone(), seen.clone());
    rig.apps.when_quitting(move |_| {
        s2.lock().unwrap().push(
            kc.get(LIVE_SERVICE, LIVE_ACCOUNT)
                .and_then(|v| decode_keyring(&v)),
        );
    });
    rig.store.switch_to("2").unwrap();
    for at_quit in seen.lock().unwrap().iter() {
        assert_eq!(
            at_quit.as_deref(),
            Some(a.as_str()),
            "the login was still the old one when the apps quit"
        );
    }
    assert_eq!(rig.live().as_deref(), Some(b.as_str()));
    assert_eq!(
        std::fs::read_to_string(rig.token_file()).unwrap(),
        b,
        "the mirror is set too"
    );
    // Our copy of the account being left holds what was live (it might have been renewed).
    assert_eq!(rig.keychain.get(SERVICE, "1").as_deref(), Some(a.as_str()));
    let events: Vec<_> = rig
        .apps
        .events()
        .into_iter()
        .filter(|e| !e.starts_with("is_running"))
        .collect();
    assert_eq!(
        events,
        vec![
            format!("quit {}", BUNDLE_IDS[0]),
            format!("quit {}", BUNDLE_IDS[1]),
            format!("open {}", BUNDLE_IDS[0]),
            format!("open {}", BUNDLE_IDS[1]),
        ]
    );
    assert_eq!(rig.store.state().active_id.as_deref(), Some("2"));
}

#[test]
fn the_login_item_is_updated_in_place_and_never_deleted_by_a_switch() {
    let rig = Rig::new("inplace", FakeApps::new());
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    rig.save("2", &fake_login("2", "b@example.com", "tb"));
    rig.store.switch_to("2").unwrap();
    let calls = rig.keychain.calls();
    assert!(!calls
        .iter()
        .any(|c| c == &format!("delete {LIVE_SERVICE}/{LIVE_ACCOUNT}")));
    assert!(calls.contains(&format!("write {LIVE_SERVICE}/{LIVE_ACCOUNT}")));
}

#[test]
fn an_email_only_login_drops_the_stale_mirror_instead_of_writing_it() {
    let rig = Rig::new("nomirror", FakeApps::new());
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    std::fs::write(rig.token_file(), "old mirror").unwrap();
    rig.save("e", &fake_email_login("e@example.com", "te"));
    rig.store.switch_to("e").unwrap();
    assert!(!rig.token_file().exists());
    assert!(rig.live().unwrap().contains("id_token_email"));
}

#[test]
fn an_app_that_will_not_quit_changes_nothing_and_reopens_what_it_closed() {
    let rig = Rig::new(
        "stubborn",
        FakeApps::new()
            .running(BUNDLE_IDS[0])
            .running(BUNDLE_IDS[1])
            .stubborn(BUNDLE_IDS[1]),
    );
    let a = fake_login("1", "a@example.com", "ta");
    rig.set_live(&a);
    rig.save("2", &fake_login("2", "b@example.com", "tb"));
    let err = rig.store.switch_to("2").unwrap_err();
    assert!(err.message().contains("didn't quit"));
    assert_eq!(rig.live().as_deref(), Some(a.as_str()));
    assert!(rig
        .apps
        .events()
        .contains(&format!("open {}", BUNDLE_IDS[0])));
    assert!(!rig.store.state().busy);
}

#[test]
fn a_switch_to_the_account_in_use_changes_nothing() {
    let rig = Rig::new("same", FakeApps::new().running(BUNDLE_IDS[0]));
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    rig.store.switch_to("1").unwrap();
    assert!(!rig.apps.events().iter().any(|e| e.starts_with("quit")));
}

#[test]
fn a_switch_to_an_unsaved_account_says_so_and_quits_nothing() {
    let rig = Rig::new("unsaved", FakeApps::new().running(BUNDLE_IDS[0]));
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    let err = rig.store.switch_to("9").unwrap_err();
    assert!(err.message().contains("no saved login"));
    assert!(!rig.apps.events().iter().any(|e| e.starts_with("quit")));
}

#[test]
fn add_account_saves_the_current_login_deletes_the_item_and_waits_for_the_next() {
    let rig = Rig::new("add", FakeApps::new().running(BUNDLE_IDS[0]));
    let a = fake_login("1", "a@example.com", "ta");
    rig.set_live(&a);
    std::fs::write(rig.token_file(), &a).unwrap();
    rig.store.begin_add().unwrap();
    assert_eq!(
        rig.keychain.get(SERVICE, "1").as_deref(),
        Some(a.as_str()),
        "kept before it goes"
    );
    assert!(
        rig.live().is_none(),
        "the item is deleted so Antigravity starts at its sign-in screen"
    );
    assert!(!rig.token_file().exists());
    let state = rig.store.state();
    assert!(state.adding && state.active_id.is_none());
    assert_eq!(
        rig.store.poll_add(),
        AddProgress::Waiting { sign_in_url: None }
    );
    // The user signs in to another account in Antigravity.
    rig.set_live(&fake_login("2", "b@example.com", "tb"));
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    let state = rig.store.state();
    assert_eq!(state.active_id.as_deref(), Some("2"));
    assert_eq!(state.accounts.len(), 2);
    assert!(!state.adding);
    assert_eq!(rig.store.poll_add(), AddProgress::Idle);
}

#[test]
fn add_account_refuses_when_the_current_login_cannot_be_kept() {
    let rig = Rig::new("addfail", FakeApps::new());
    let a = fake_login("1", "a@example.com", "ta");
    rig.set_live(&a);
    rig.keychain.fail_writes();
    let err = rig.store.begin_add().unwrap_err();
    assert!(err.message().contains("left in place"));
    assert_eq!(rig.live().as_deref(), Some(a.as_str()));
    assert!(!rig.apps.events().iter().any(|e| e.starts_with("quit")));
}

#[test]
fn cancelling_an_add_puts_the_previous_login_back() {
    let rig = Rig::new("cancel", FakeApps::new());
    let a = fake_login("1", "a@example.com", "ta");
    rig.set_live(&a);
    rig.store.begin_add().unwrap();
    rig.store.cancel_add();
    assert_eq!(rig.live().as_deref(), Some(a.as_str()));
    assert_eq!(rig.store.state().active_id.as_deref(), Some("1"));
    assert!(!rig.store.state().adding);
}

#[test]
fn limits_come_from_the_local_server_for_the_active_account() {
    let procs = FakeProcs::new()
        .answer(
            "/usr/bin/pgrep -f bin/language_server --standalone",
            "4242\n",
        )
        .answer("/bin/ps -o command= -p 4242", "x --csrf_token tok\n")
        .answer(
            "/usr/sbin/lsof -nP -a -p 4242 -iTCP -sTCP:LISTEN",
            "l 4242 me 1u IPv4 0 0t0 TCP 127.0.0.1:5000 (LISTEN)\n",
        );
    let rig = Rig::with_procs("limits", procs, FakeApps::new());
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    rig.http.ok(
        200,
        r#"{"userStatus":{"email":"a@example.com","userTier":{"name":"Antigravity Pro Quota"}}}"#,
    );
    rig.http.ok(200, r#"{"response":{"groups":[{"displayName":"G","buckets":[{"window":"weekly","remainingFraction":0.5}]}]}}"#);
    rig.store.sync(false);
    let report = rig.store.usage_report("1").unwrap();
    assert_eq!(report.bars()[0].percent, 50.0);
    assert_eq!(rig.store.state().accounts[0].plan.as_deref(), Some("Pro"));
    assert!(rig
        .http
        .requests()
        .iter()
        .all(|r| r.url.starts_with("https://127.0.0.1:5000/")));
}

#[test]
fn removing_forgets_our_copy_and_leaves_antigravitys_item() {
    let rig = Rig::new("remove", FakeApps::new());
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    rig.store.sync(false);
    rig.store.remove("1");
    assert!(rig.keychain.get(SERVICE, "1").is_none());
    assert!(rig.live().is_some());
    assert!(rig.store.state().accounts.is_empty());
}

#[test]
fn a_restart_confirmation_only_while_antigravity_runs() {
    let rig = Rig::new("confirm", FakeApps::new());
    assert!(rig.store.switch_confirmation("x").is_none());
    let rig2 = Rig::new("confirm2", FakeApps::new().running(BUNDLE_IDS[1]));
    assert_eq!(
        rig2.store.switch_confirmation("x").unwrap().title,
        "Restart Antigravity?"
    );
}
