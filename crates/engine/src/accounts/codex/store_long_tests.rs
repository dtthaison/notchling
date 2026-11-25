use super::store::{APP_BUNDLE_ID, SERVICE};
use super::store_tests::{Rig, FUTURE};
use super::testkit::{big_login_for, login_for};
use crate::accounts::chunked::{self, part_name, Plain};
use crate::accounts::generic_password::{FakeKeychain, Keychain};
use crate::accounts::procs::{FakeApps, FakeProcs, ProcCall};
use crate::providers::{AccountProvider, AddProgress};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::path::PathBuf;
use std::sync::Arc;

fn rig(tag: &str) -> Rig {
    Rig::new(
        tag,
        FakeProcs::new(),
        FakeApps::new().running(APP_BUNDLE_ID),
    )
}

fn saved_copy(rig: &Rig, id: &str) -> Option<Vec<u8>> {
    chunked::read(&Plain(rig.keychain.as_ref()), SERVICE, id)
        .ok()
        .flatten()
        .and_then(|v| STANDARD.decode(v).ok())
}

fn save_split(rig: &Rig, id: &str, login: &[u8]) {
    chunked::write(
        &Plain(rig.keychain.as_ref()),
        SERVICE,
        id,
        &STANDARD.encode(login),
    )
    .unwrap();
}

#[test]
fn a_real_sized_live_login_is_saved_before_the_switch_and_comes_back_whole() {
    for size in [3_000, 4_500, 6_000] {
        let rig = rig(&format!("long-big-{size}"));
        let a = big_login_for("user-1", "ws-1", "rt-a", size);
        let b = big_login_for("user-2", "ws-2", "rt-b", size);
        assert!(a.len() > 4_000, "{} bytes", a.len());
        save_split(&rig, "user-2:ws-2", &b);
        rig.live(&a);

        rig.store.switch_to("user-2:ws-2").unwrap();

        assert_eq!(std::fs::read(rig.auth()).unwrap(), b);
        assert_eq!(
            saved_copy(&rig, "user-1:ws-1").as_deref(),
            Some(a.as_slice())
        );
        assert!(rig
            .keychain
            .get(SERVICE, &part_name("user-1:ws-1", 0))
            .is_some());
        rig.store.switch_to("user-1:ws-1").unwrap();
        assert_eq!(std::fs::read(rig.auth()).unwrap(), a);
    }
}

#[test]
fn a_live_login_that_cannot_be_saved_is_never_overwritten() {
    let rig = rig("long-unsaved");
    let a = big_login_for("user-1", "ws-1", "rt-a", 4_500);
    let b = big_login_for("user-2", "ws-2", "rt-b", 4_500);
    save_split(&rig, "user-2:ws-2", &b);
    rig.live(&a);
    rig.keychain.fail_writes();

    let err = rig.store.switch_to("user-2:ws-2").unwrap_err();

    assert!(
        err.message().contains("couldn't be saved"),
        "{}",
        err.message()
    );
    assert_eq!(std::fs::read(rig.auth()).unwrap(), a, "auth.json untouched");
    assert!(!rig.apps.events().iter().any(|e| e.starts_with("quit")));
    assert!(!rig.store.state().busy);
}

#[test]
fn a_live_login_naming_no_account_is_never_overwritten() {
    let rig = rig("long-anon");
    let b = big_login_for("user-2", "ws-2", "rt-b", 4_500);
    save_split(&rig, "user-2:ws-2", &b);
    let api_key_login = br#"{"OPENAI_API_KEY": "sk-FAKE-not-a-key", "tokens": null}"#;
    rig.live(api_key_login);

    let err = rig.store.switch_to("user-2:ws-2").unwrap_err();

    assert!(
        err.message().contains("couldn't be saved"),
        "{}",
        err.message()
    );
    assert_eq!(std::fs::read(rig.auth()).unwrap(), api_key_login);
    assert!(!rig.apps.events().iter().any(|e| e.starts_with("quit")));
}

#[test]
fn a_damaged_saved_copy_is_refused_and_nothing_changes() {
    let rig = rig("long-damaged");
    let a = big_login_for("user-1", "ws-1", "rt-a", 4_500);
    let b = big_login_for("user-2", "ws-2", "rt-b", 4_500);
    save_split(&rig, "user-2:ws-2", &b);
    let part = rig
        .keychain
        .get(SERVICE, &part_name("user-2:ws-2", 1))
        .unwrap();
    rig.keychain
        .write(
            SERVICE,
            &part_name("user-2:ws-2", 1),
            &format!("QUFB{}", &part[4..]),
        )
        .unwrap();
    rig.live(&a);

    assert!(rig.store.switch_to("user-2:ws-2").is_err());

    assert_eq!(std::fs::read(rig.auth()).unwrap(), a);
    assert!(!rig.apps.events().iter().any(|e| e.starts_with("quit")));
}

#[test]
fn a_long_one_item_copy_the_swift_app_saved_still_switches() {
    let a = big_login_for("user-1", "ws-1", "rt-a", 4_500);
    let b = big_login_for("user-2", "ws-2", "rt-b", 4_500);
    let keychain = FakeKeychain::new().with(SERVICE, "user-2:ws-2", &STANDARD.encode(&b));
    let root = std::env::temp_dir().join(format!("nl-codex-swift-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("codex")).unwrap();
    let rig = Rig::at(
        root,
        Arc::new(keychain),
        FakeProcs::new(),
        FakeApps::new().running(APP_BUNDLE_ID),
    );
    rig.live(&a);

    rig.store.switch_to("user-2:ws-2").unwrap();

    assert_eq!(std::fs::read(rig.auth()).unwrap(), b);
    assert_eq!(
        saved_copy(&rig, "user-1:ws-1").as_deref(),
        Some(a.as_slice())
    );
}

#[test]
fn an_add_names_the_account_its_own_sign_in_ended_on_not_the_login_in_use() {
    let procs = FakeProcs::new().binary("codex", &PathBuf::from("/usr/local/bin/codex"));
    let rig = Rig::new("long-named", procs, FakeApps::new());
    assert_eq!(rig.store.added_account(), Some(None));
    rig.store.begin_add().unwrap();
    let scratch = rig
        .procs
        .calls()
        .into_iter()
        .find_map(|c| match c {
            ProcCall::SpawnLogin(s) => Some(PathBuf::from(&s.env[0].1)),
            _ => None,
        })
        .unwrap();
    rig.live(&login_for(
        "t@example.com",
        "user-8",
        "ws-8",
        FUTURE,
        "rt-t",
    ));
    let c = login_for("c@example.com", "user-3", "ws-3", FUTURE, "rt-c");
    std::fs::write(scratch.join("auth.json"), &c).unwrap();
    let script = rig.procs.login_state();
    script.lock().unwrap().exit = Some(0);
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(
        rig.store.added_account().flatten().as_deref(),
        Some("user-3:ws-3")
    );
    let state = rig.store.state();
    assert_eq!(state.active_id.as_deref(), Some("user-8:ws-8"));
    assert_eq!(state.accounts.len(), 2, "the terminal login is listed too");

    script.lock().unwrap().exit = None;
    rig.store.begin_add().unwrap();
    assert_eq!(
        rig.store.added_account(),
        Some(None),
        "a new add has ended on nothing yet"
    );
    rig.store.cancel_add();
    assert_eq!(rig.store.poll_add(), AddProgress::Cancelled);
    assert_eq!(rig.store.added_account(), Some(None));
}
