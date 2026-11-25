use super::files::{backup, cookies};
use super::store::{ClaudeName, DesktopDeps, DesktopPaths, DesktopStore, BUNDLE_ID};
use crate::accounts::procs::FakeApps;
use crate::providers::{AccountProvider, AddProgress};
use std::path::PathBuf;
use std::sync::Arc;

struct Rig {
    root: PathBuf,
    apps: Arc<FakeApps>,
    store: DesktopStore,
}

fn claude_names() -> Vec<ClaudeName> {
    vec![
        ClaudeName {
            id: "uuid-a:org-1".into(),
            email: "a@example.com".into(),
        },
        ClaudeName {
            id: "uuid-b:org-2".into(),
            email: "b@example.com".into(),
        },
        ClaudeName {
            id: "uuid-c:org-3".into(),
            email: "c@example.com".into(),
        },
    ]
}

impl Rig {
    fn new(tag: &str, apps: FakeApps) -> Rig {
        let root = std::env::temp_dir().join(format!("nl-desktop-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Claude")).unwrap();
        Rig::at(root, apps)
    }

    fn at(root: PathBuf, apps: FakeApps) -> Rig {
        let apps = Arc::new(apps);
        let store = DesktopStore::new(
            DesktopDeps {
                apps: apps.clone(),
                claude_accounts: Arc::new(claude_names),
            },
            DesktopPaths {
                claude_dir: root.join("Claude"),
                store_dir: root.join("ours/desktop"),
                apps_dir: root.join("Applications"),
            },
        );
        Rig { root, apps, store }
    }

    fn claude(&self) -> PathBuf {
        self.root.join("Claude")
    }

    fn sign_in(&self, uuid: &str, cookies_bytes: &[u8]) {
        std::fs::write(cookies(&self.claude()), cookies_bytes).unwrap();
        std::fs::write(
            self.claude().join("config.json"),
            format!(r#"{{"lastKnownAccountUuid":"{uuid}","theme":"dark"}}"#),
        )
        .unwrap();
    }

    fn ours(&self, uuid: &str) -> PathBuf {
        self.root.join(format!("ours/desktop/{uuid}.cookies"))
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn sync_captures_the_active_accounts_cookies_and_lists_only_accounts_in_desktop() {
    let rig = Rig::new("sync", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.sync(false);
    assert_eq!(std::fs::read(rig.ours("uuid-a")).unwrap(), b"cookies-a");
    let state = rig.store.state();
    assert_eq!(state.active_id.as_deref(), Some("uuid-a"));
    let rows: Vec<_> = state
        .accounts
        .iter()
        .map(|a| (a.id.as_str(), a.switchable))
        .collect();
    assert_eq!(
        rows,
        vec![("uuid-a", true)],
        "b and c were never signed in to Desktop"
    );
    assert_eq!(state.active_name().as_deref(), Some("a@example.com"));
}

#[test]
fn a_desktop_account_with_no_claude_code_login_is_named_signed_in() {
    let rig = Rig::new("unlisted", FakeApps::new());
    rig.sign_in("uuid-zzz", b"cookies-z");
    rig.store.sync(false);
    let state = rig.store.state();
    assert!(state.accounts.is_empty());
    assert_eq!(state.active_name().as_deref(), Some("Signed in"));
}

#[test]
fn a_switch_quits_desktop_first_then_swaps_the_whole_cookie_store() {
    let rig = Rig::new("switch", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.sync(false);
    std::fs::create_dir_all(rig.root.join("ours/desktop")).unwrap();
    std::fs::write(rig.ours("uuid-b"), b"cookies-b").unwrap();
    let rig = Rig::at(rig.root.clone(), FakeApps::new().running(BUNDLE_ID));
    let seen = Arc::new(std::sync::Mutex::new(None));
    let (live, s2) = (cookies(&rig.claude()), seen.clone());
    rig.apps
        .when_quitting(move |_| *s2.lock().unwrap() = std::fs::read(&live).ok());

    rig.store.switch_to("uuid-b").unwrap();

    assert_eq!(
        seen.lock().unwrap().as_deref(),
        Some(&b"cookies-a"[..]),
        "still account a's when Desktop quit"
    );
    assert_eq!(std::fs::read(cookies(&rig.claude())).unwrap(), b"cookies-b");
    assert_eq!(std::fs::read(backup(&rig.claude())).unwrap(), b"cookies-a");
    let config = std::fs::read_to_string(rig.claude().join("config.json")).unwrap();
    assert!(
        config.contains("uuid-b") && config.contains("bootSideSessions") && config.contains("dark")
    );
    let events: Vec<_> = rig
        .apps
        .events()
        .into_iter()
        .filter(|e| !e.starts_with("is_running"))
        .collect();
    assert_eq!(
        events,
        vec![format!("quit {BUNDLE_ID}"), format!("open {BUNDLE_ID}")]
    );
    assert_eq!(rig.store.state().active_id.as_deref(), Some("uuid-b"));
}

#[test]
fn desktop_that_will_not_quit_keeps_its_cookies() {
    let rig = Rig::new(
        "stubborn",
        FakeApps::new().running(BUNDLE_ID).stubborn(BUNDLE_ID),
    );
    rig.sign_in("uuid-a", b"cookies-a");
    std::fs::create_dir_all(rig.root.join("ours/desktop")).unwrap();
    std::fs::write(rig.ours("uuid-b"), b"cookies-b").unwrap();
    let rig = Rig::at(
        rig.root.clone(),
        FakeApps::new().running(BUNDLE_ID).stubborn(BUNDLE_ID),
    );
    let err = rig.store.switch_to("uuid-b").unwrap_err();
    assert!(err.message().contains("didn't quit"));
    assert_eq!(std::fs::read(cookies(&rig.claude())).unwrap(), b"cookies-a");
    assert!(!backup(&rig.claude()).exists());
    assert!(!rig.store.state().busy);
}

#[test]
fn a_switch_to_an_account_never_signed_in_asks_for_that_first() {
    let rig = Rig::new("nosession", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    let err = rig.store.switch_to("uuid-b").unwrap_err();
    assert!(err.message().contains("Sign in to it in Claude once"));
    assert!(!rig.apps.events().iter().any(|e| e.starts_with("quit")));
    for bad in ["../Claude/Cookies", "a/b", ""] {
        assert!(rig.store.switch_to(bad).is_err(), "{bad}");
    }
    assert_eq!(std::fs::read(cookies(&rig.claude())).unwrap(), b"cookies-a");
}

#[test]
fn add_account_saves_this_one_signs_desktop_out_and_waits_for_the_next() {
    let rig = Rig::new("add", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    assert_eq!(std::fs::read(rig.ours("uuid-a")).unwrap(), b"cookies-a");
    assert!(!cookies(&rig.claude()).exists(), "signed out");
    assert_eq!(std::fs::read(backup(&rig.claude())).unwrap(), b"cookies-a");
    let config = std::fs::read_to_string(rig.claude().join("config.json")).unwrap();
    assert!(!config.contains("lastKnownAccountUuid") && config.contains("dark"));
    let state = rig.store.state();
    assert!(state.adding && state.active_id.is_none());
    assert_eq!(
        state
            .accounts
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        vec!["uuid-a"]
    );
    assert_eq!(
        rig.store.poll_add(),
        AddProgress::Waiting { sign_in_url: None }
    );
    rig.sign_in("uuid-b", b"cookies-b");
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(std::fs::read(rig.ours("uuid-b")).unwrap(), b"cookies-b");
    assert_eq!(rig.store.state().active_id.as_deref(), Some("uuid-b"));
    assert_eq!(rig.store.poll_add(), AddProgress::Idle);
}

#[test]
fn cancelling_an_add_puts_the_previous_account_back() {
    let rig = Rig::new("cancel", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    rig.store.cancel_add();
    assert_eq!(std::fs::read(cookies(&rig.claude())).unwrap(), b"cookies-a");
    assert_eq!(rig.store.state().active_id.as_deref(), Some("uuid-a"));
    assert!(!rig.store.state().adding);
}

#[test]
fn forgetting_deletes_only_our_copy() {
    let rig = Rig::new("forget", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.sync(false);
    rig.store.remove("uuid-a");
    assert!(!rig.ours("uuid-a").exists());
    assert_eq!(std::fs::read(cookies(&rig.claude())).unwrap(), b"cookies-a");
    rig.store.remove("../Claude/Cookies");
    assert!(cookies(&rig.claude()).exists());
}

#[test]
fn confirmations_only_while_desktop_runs_and_availability_follows_the_app() {
    let rig = Rig::new("confirm", FakeApps::new());
    assert!(rig.store.switch_confirmation("x").is_none() && rig.store.add_confirmation().is_none());
    assert!(!rig.store.is_available());
    std::fs::create_dir_all(rig.root.join("Applications/Claude.app")).unwrap();
    assert!(rig.store.is_available());
    let running = Rig::new("confirm2", FakeApps::new().running(BUNDLE_ID));
    assert_eq!(
        running.store.switch_confirmation("x").unwrap().title,
        "Restart Claude Desktop?"
    );
    assert_eq!(
        running.store.add_confirmation().unwrap().title,
        "Open Claude Desktop signed out?"
    );
}
