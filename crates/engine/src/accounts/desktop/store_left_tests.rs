use super::files::cookies;
use super::store::{ClaudeName, DesktopDeps, DesktopPaths, DesktopStore, BUNDLE_ID};
use super::store_add_tests::AddRig;
use super::store_keep_tests::live;
use crate::accounts::procs::FakeApps;
use crate::providers::{AccountProvider, AddProgress};
use std::sync::Arc;

fn half_signed_in(rig: &AddRig, pending: &str) {
    rig.sign_in("uuid-b", b"half-written");
    let pending = rig.claude().join(pending);
    std::fs::write(pending, b"the login itself").unwrap();
}

#[test]
fn a_cancel_that_leaves_a_login_being_written_never_says_desktop_was_left_signed_out() {
    for apps in ["closed", "stubborn"] {
        let fake = match apps {
            "stubborn" => FakeApps::new().stubborn(BUNDLE_ID),
            _ => FakeApps::new(),
        };
        let rig = AddRig::new(&format!("left-writing-{apps}"), fake);
        rig.sign_in("uuid-a", b"cookies-a");
        rig.store.begin_add().unwrap();
        half_signed_in(&rig, "Cookies-wal");
        rig.store.cancel_add();
        assert_eq!(live(&rig), b"half-written", "{apps}");
        assert_eq!(rig.store.abandon_add(), None, "{apps}");
        assert!(!rig.store.state().adding, "{apps}");
    }
}

#[test]
fn a_cancel_whose_restore_is_refused_after_desktop_finished_a_login_names_that_login() {
    let rig = AddRig::new("left-flushed", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    rig.block_copy("uuid-b");
    half_signed_in(&rig, "Cookies-journal");
    let (live_cookies, pending) = (cookies(&rig.claude()), rig.claude().join("Cookies-journal"));
    rig.apps.when_quitting(move |_| {
        std::fs::write(&live_cookies, b"cookies-b").unwrap();
        std::fs::write(&pending, b"").unwrap();
    });
    rig.store.cancel_add();
    assert_eq!(live(&rig), b"cookies-b", "the restore was refused");
    assert!(rig.store.is_running(), "Desktop was opened again");
    let state = rig.store.state();
    assert_eq!(state.active_id.as_deref(), Some("uuid-b"));
    assert!(!state.adding);
    assert_eq!(rig.store.abandon_add(), None);
}

#[test]
fn a_sync_while_an_add_is_open_leaves_the_add_to_its_poll() {
    let rig = AddRig::new("sync-in-add", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    rig.sign_in("uuid-b", b"cookies-b");
    rig.store.sync(false);
    let state = rig.store.state();
    assert!(state.adding);
    assert_eq!(state.active_id, None, "the add's poll names it");
    assert!(!rig.ours("uuid-b").exists(), "the add's poll saves it");
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(rig.store.added_account(), Some(Some("uuid-b".to_string())));
    assert_eq!(rig.store.state().active_id.as_deref(), Some("uuid-b"));
    assert_eq!(rig.store.abandon_add(), None);
}

#[test]
fn an_abandon_never_says_desktop_was_left_signed_out_while_a_named_login_is_live() {
    for synced in [true, false] {
        let rig = AddRig::new(&format!("abandon-live-{synced}"), FakeApps::new());
        rig.sign_in("uuid-a", b"cookies-a");
        rig.store.begin_add().unwrap();
        rig.sign_in("uuid-b", b"cookies-b");
        if synced {
            rig.store.sync(false);
        }
        assert_eq!(rig.store.abandon_add(), None, "synced {synced}");
        assert!(!rig.store.state().adding, "synced {synced}");
        assert_eq!(rig.store.abandon_add(), None, "synced {synced}");
    }
}

#[test]
fn a_change_that_fails_after_keeping_the_login_still_lists_the_copy_it_kept() {
    let tag = format!("nl-desktop-kept-failed-{}", std::process::id());
    let root = std::env::temp_dir().join(tag);
    let _ = std::fs::remove_dir_all(&root);
    let claude = root.join("Claude");
    std::fs::create_dir_all(&claude).unwrap();
    let apps = Arc::new(FakeApps::new());
    let named = || {
        vec![ClaudeName {
            id: "uuid-a:org".into(),
            email: "a@example.com".into(),
        }]
    };
    let store = DesktopStore::new(
        DesktopDeps {
            apps: apps.clone(),
            claude_accounts: Arc::new(named),
        },
        DesktopPaths {
            claude_dir: claude.clone(),
            store_dir: claude.join("Cookies-shm"),
            apps_dir: root.join("Applications"),
        },
    );
    let rig = AddRig {
        root,
        apps,
        store: Arc::new(store),
    };
    rig.sign_in("uuid-a", b"cookies-a");
    assert!(!rig.store.is_available());
    assert!(rig.store.begin_add().is_err());
    let copy = claude.join("Cookies-shm/uuid-a.cookies");
    assert_eq!(std::fs::read(copy).unwrap(), b"cookies-a", "kept");
    assert_eq!(live(&rig), b"cookies-a", "and left in place");
    assert!(!rig.store.state().adding);
    let rows: Vec<_> = rig
        .store
        .state()
        .accounts
        .into_iter()
        .map(|a| (a.id, a.switchable))
        .collect();
    assert_eq!(rows, [("uuid-a".to_string(), true)]);
    assert!(rig.store.is_available());
}
