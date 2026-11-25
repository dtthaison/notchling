use super::files::cookies;
use super::store::BUNDLE_ID;
use super::store_add_tests::AddRig;
use super::store_keep_tests::{kept, live};
use crate::accounts::procs::FakeApps;
use crate::providers::{AccountProvider, AddProgress};
use std::sync::mpsc;
use std::time::Duration;

const WRITE_AHEAD: [&str; 2] = ["Cookies-wal", "Cookies-journal"];
const WAITING: AddProgress = AddProgress::Waiting { sign_in_url: None };

fn seed_unnamed(rig: &AddRig, copies: u64) {
    let dir = rig.root.join("ours/desktop/unnamed");
    std::fs::create_dir_all(&dir).unwrap();
    for n in 1..=copies {
        let name = format!("{n:012}.cookies");
        std::fs::write(dir.join(name), format!("unnamed-{n}")).unwrap();
    }
}

fn sign_in_unnamed(rig: &AddRig, bytes: &[u8]) {
    rig.sign_out();
    std::fs::write(cookies(&rig.claude()), bytes).unwrap();
}

fn named(rig: &AddRig) -> Option<String> {
    rig.store.state().active_id
}

fn saved(rig: &AddRig, uuid: &str) -> Vec<u8> {
    std::fs::read(rig.ours(uuid)).unwrap_or_default()
}

#[test]
fn a_change_never_cuts_the_unnamed_copy_it_just_kept() {
    let rig = AddRig::new("prune-kept", FakeApps::new());
    seed_unnamed(&rig, 11);
    sign_in_unnamed(&rig, b"unnamed-1");
    rig.store.begin_add().unwrap();
    assert!(!cookies(&rig.claude()).exists());
    let held = kept(&rig);
    assert!(held.contains(&b"unnamed-1".to_vec()), "the login is gone");
    assert_eq!(held.len(), 10);
    assert!(!held.contains(&b"unnamed-2".to_vec()), "the oldest goes");
    rig.store.cancel_add();
    assert!(cookies(&rig.claude()).exists(), "left signed out");
    assert_eq!(live(&rig), b"unnamed-1");
    assert_eq!(rig.store.abandon_add(), None);
}

#[test]
fn an_unnamed_login_kept_again_is_the_newest_copy() {
    let rig = AddRig::new("prune-newest", FakeApps::new());
    seed_unnamed(&rig, 10);
    sign_in_unnamed(&rig, b"unnamed-1");
    rig.store.begin_add().unwrap();
    rig.store.abandon_add();
    assert_eq!(kept(&rig).len(), 10);
    sign_in_unnamed(&rig, b"unnamed-new");
    rig.store.begin_add().unwrap();
    let held = kept(&rig);
    assert_eq!(held.len(), 10);
    assert_eq!(held[9], b"unnamed-new");
    assert_eq!(held[8], b"unnamed-1", "used last before it");
    assert_eq!(held[0], b"unnamed-3", "the copies not used since go first");
}

#[test]
fn a_sync_never_saves_a_login_desktop_is_still_writing() {
    let rig = AddRig::new("sync-unsettled", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.sync(false);
    assert_eq!(saved(&rig, "uuid-a"), b"cookies-a");
    for name in WRITE_AHEAD {
        let pending = rig.claude().join(name);
        std::fs::write(cookies(&rig.claude()), b"half-written").unwrap();
        std::fs::write(&pending, b"the rest of it").unwrap();
        rig.store.sync(false);
        assert_eq!(saved(&rig, "uuid-a"), b"cookies-a", "{name}");
        assert_eq!(named(&rig).as_deref(), Some("uuid-a"), "{name}");
        rig.sign_in("uuid-b", b"half-written");
        rig.store.sync(false);
        assert!(!rig.ours("uuid-b").exists(), "{name}");
        assert_eq!(named(&rig).as_deref(), Some("uuid-a"), "not known yet");
        std::fs::remove_file(&pending).unwrap();
        rig.sign_in("uuid-a", b"half-written");
    }
    std::fs::write(cookies(&rig.claude()), b"cookies-a-2").unwrap();
    rig.store.sync(false);
    assert_eq!(saved(&rig, "uuid-a"), b"cookies-a-2");
}

#[test]
fn an_add_waits_while_desktop_is_still_writing_the_new_login() {
    for name in WRITE_AHEAD {
        let rig = AddRig::new(&format!("poll-{name}"), FakeApps::new().running(BUNDLE_ID));
        rig.sign_in("uuid-a", b"cookies-a");
        rig.store.begin_add().unwrap();
        rig.sign_in("uuid-b", b"half-written");
        let pending = rig.claude().join(name);
        std::fs::write(&pending, b"the login itself").unwrap();
        assert_eq!(rig.store.poll_add(), WAITING, "{name}");
        assert!(!rig.ours("uuid-b").exists(), "{name}");
        assert!(rig.store.state().adding);
        std::fs::remove_file(&pending).unwrap();
        std::fs::write(cookies(&rig.claude()), b"cookies-b").unwrap();
        assert_eq!(rig.store.poll_add(), AddProgress::Done);
        assert_eq!(saved(&rig, "uuid-b"), b"cookies-b");
        assert_eq!(rig.store.added_account(), Some(Some("uuid-b".to_string())));
    }
}

#[test]
fn a_cancel_saves_and_names_a_login_finished_just_before_it() {
    let rig = AddRig::new("cancel-finished", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    rig.sign_in("uuid-b", b"cookies-b");
    let opens = rig.opens();
    rig.store.cancel_add();
    assert_eq!(live(&rig), b"cookies-b", "left as the user made it");
    assert_eq!(rig.opens(), opens, "Desktop is not restarted");
    assert_eq!(saved(&rig, "uuid-b"), b"cookies-b");
    assert_eq!(named(&rig).as_deref(), Some("uuid-b"));
    assert_eq!(rig.ids(), ["uuid-a", "uuid-b"]);
    assert_eq!(rig.store.abandon_add(), None);
    assert!(!rig.store.state().adding);

    let rig = AddRig::new("cancel-finished-out", FakeApps::new());
    rig.sign_out();
    rig.store.begin_add().unwrap();
    rig.sign_in("uuid-b", b"cookies-b");
    rig.store.cancel_add();
    assert_eq!(saved(&rig, "uuid-b"), b"cookies-b");
    assert_eq!(named(&rig).as_deref(), Some("uuid-b"));
    assert_eq!(live(&rig), b"cookies-b");
}

#[test]
fn a_cancel_keeps_a_login_desktop_finishes_writing_as_it_quits() {
    let rig = AddRig::new("cancel-writing", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    rig.sign_in("uuid-b", b"half-written");
    let pending = rig.claude().join("Cookies-journal");
    std::fs::write(&pending, b"the login itself").unwrap();
    let live_cookies = cookies(&rig.claude());
    rig.apps.when_quitting(move |_| {
        std::fs::write(&live_cookies, b"cookies-b").unwrap();
        std::fs::write(&pending, b"").unwrap();
    });
    rig.store.cancel_add();
    assert_eq!(saved(&rig, "uuid-b"), b"cookies-b", "kept whole");
    assert_eq!(live(&rig), b"cookies-a", "the old account is back");
    assert_eq!(named(&rig).as_deref(), Some("uuid-a"));
    assert_eq!(rig.store.abandon_add(), None);
}

#[test]
fn a_cancel_leaves_a_login_still_being_written_when_it_cannot_settle() {
    let rig = AddRig::new("cancel-unsettled", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    rig.sign_in("uuid-b", b"half-written");
    let pending = rig.claude().join("Cookies-wal");
    std::fs::write(&pending, b"the login itself").unwrap();
    rig.store.cancel_add();
    assert_eq!(live(&rig), b"half-written");
    assert_eq!(std::fs::read(&pending).unwrap(), b"the login itself");
    assert!(!rig.ours("uuid-b").exists(), "half a login is no copy");
    assert_eq!(named(&rig), None);
    assert!(!rig.store.state().adding);
}

#[test]
fn an_add_over_a_login_still_being_written_leaves_its_saved_copy_alone() {
    let rig = AddRig::new("add-unsettled", FakeApps::new());
    rig.keep_copy("uuid-a", b"cookies-a");
    rig.sign_in("uuid-a", b"half-written");
    let pending = rig.claude().join("Cookies-journal");
    std::fs::write(&pending, b"the rest of it").unwrap();
    rig.store.sync(false);
    assert!(rig.store.begin_add().is_err());
    assert_eq!(saved(&rig, "uuid-a"), b"cookies-a");
    assert_eq!(live(&rig), b"half-written");
    assert_eq!(std::fs::read(&pending).unwrap(), b"the rest of it");
    assert!(!rig.store.state().adding);
}

#[test]
fn a_sync_over_a_config_json_desktop_has_not_finished_writing_keeps_the_account() {
    let rig = AddRig::new("sync-config", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.sync(false);
    let config = rig.claude().join("config.json");
    for bad in ["", r#"{"lastKnownAccountUuid":"#, "[]"] {
        std::fs::write(cookies(&rig.claude()), b"cookies-a-2").unwrap();
        std::fs::write(&config, bad).unwrap();
        rig.store.sync(false);
        assert_eq!(named(&rig).as_deref(), Some("uuid-a"), "{bad:?}");
        assert_eq!(saved(&rig, "uuid-a"), b"cookies-a", "{bad:?}");
    }
    std::fs::remove_file(&config).unwrap();
    std::fs::create_dir(&config).unwrap();
    rig.store.sync(false);
    assert_eq!(named(&rig).as_deref(), Some("uuid-a"), "a folder");
    std::fs::remove_dir(&config).unwrap();
    rig.store.sync(false);
    assert_eq!(named(&rig), None, "no config.json names no account");
}

#[test]
fn a_cancel_over_a_config_json_desktop_never_finished_keeps_no_unnamed_copy() {
    let rig = AddRig::new("cancel-config", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    rig.sign_in("uuid-b", b"cookies-b");
    let config = rig.claude().join("config.json");
    std::fs::write(config, r#"{"lastKnownAccountUuid":"uu"#).unwrap();
    rig.store.cancel_add();
    let unnamed = rig.root.join("ours/desktop/unnamed");
    assert!(!unnamed.exists(), "a named login is no unnamed copy");
    assert_eq!(live(&rig), b"cookies-b", "left in place");
    assert!(!rig.ours("uuid-b").exists());
    assert_eq!(saved(&rig, "uuid-a"), b"cookies-a");
}

#[test]
fn a_switch_is_refused_while_an_add_is_open_and_a_cancel_still_puts_the_login_back() {
    let rig = AddRig::new("switch-in-add", FakeApps::new());
    rig.keep_copy("uuid-b", b"cookies-b");
    seed_unnamed(&rig, 10);
    sign_in_unnamed(&rig, b"unnamed-old");
    rig.store.begin_add().unwrap();
    std::fs::write(cookies(&rig.claude()), b"half-signed-in").unwrap();
    let events = rig.apps.events().len();
    let err = rig.store.switch_to("uuid-b").unwrap_err();
    assert!(err.message().contains("sign-in"), "{err}");
    assert_eq!(rig.apps.events().len(), events, "Desktop was left alone");
    assert_eq!(live(&rig), b"half-signed-in");
    assert!(rig.store.state().adding);
    rig.store.cancel_add();
    assert_eq!(live(&rig), b"unnamed-old");
    assert_eq!(rig.store.abandon_add(), None);
    rig.store.switch_to("uuid-b").unwrap();
    assert_eq!(live(&rig), b"cookies-b");
}

#[test]
fn a_second_begin_while_an_add_is_open_is_that_add_and_a_cancel_puts_the_first_login_back() {
    let rig = AddRig::new("begin-in-add", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    let events = rig.apps.events().len();
    rig.store.begin_add().unwrap();
    assert!(rig.store.state().adding);
    let again = rig.apps.events().len();
    rig.store.cancel_add();
    assert!(cookies(&rig.claude()).exists(), "left signed out");
    assert_eq!(live(&rig), b"cookies-a");
    assert_eq!(named(&rig).as_deref(), Some("uuid-a"));
    assert_eq!(again, events, "Desktop was left alone");
}

#[test]
fn an_abandon_waits_for_a_begin_that_is_running_and_ends_the_add_it_opens() {
    let rig = AddRig::new("abandon-beginning", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    let (entered, quitting) = mpsc::channel();
    let (go, gate) = mpsc::channel::<()>();
    rig.apps.when_quitting(move |_| {
        let _ = entered.send(());
        let _ = gate.recv();
    });
    let store = rig.store.clone();
    let begin = std::thread::spawn(move || store.begin_add().is_ok());
    quitting.recv().unwrap();
    let (store, (told, abandoned)) = (rig.store.clone(), mpsc::channel());
    let abandon = std::thread::spawn(move || told.send(store.abandon_add()).is_ok());
    let early = abandoned.recv_timeout(Duration::from_millis(300));
    assert!(early.is_err(), "it answered while the begin ran: {early:?}");
    drop(go);
    assert!(begin.join().unwrap() && abandon.join().unwrap());
    let note = abandoned.recv().unwrap().unwrap_or_default();
    assert!(note.contains("left signed out"), "{note:?}");
    assert!(!rig.store.state().adding, "no add is left open");
    assert_eq!(rig.store.poll_add(), AddProgress::Idle);
}

#[test]
fn a_login_put_in_place_later_ends_the_note_that_desktop_was_left_signed_out() {
    for later in ["switch", "sync", "nothing"] {
        let rig = AddRig::new(&format!("left-{later}"), FakeApps::new());
        rig.sign_in("uuid-a", b"cookies-a");
        rig.keep_copy("uuid-b", b"cookies-b");
        rig.store.begin_add().unwrap();
        std::fs::remove_file(rig.ours("uuid-a")).unwrap();
        rig.store.cancel_add();
        assert!(
            !cookies(&rig.claude()).exists(),
            "the cancel put nothing back"
        );
        match later {
            "switch" => rig.store.switch_to("uuid-b").unwrap(),
            "sync" => rig.sign_in("uuid-b", b"cookies-b"),
            _ => {}
        }
        rig.store.sync(false);
        let note = rig.store.abandon_add();
        assert_eq!(note.is_some(), later == "nothing", "{later} {note:?}");
    }
}
