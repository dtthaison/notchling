//! Codex logins the size of real ones (4–8 KB, too long for one Keychain line), and the rule that
//! a switch never writes `auth.json` unless the login it replaces is safely saved.

use super::store::{APP_BUNDLE_ID, SERVICE};
use super::store_tests::Rig;
use super::testkit::big_login_for;
use crate::accounts::chunked::{self, part_name, Plain};
use crate::accounts::generic_password::{FakeKeychain, Keychain};
use crate::accounts::procs::{FakeApps, FakeProcs};
use crate::providers::AccountProvider;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::sync::Arc;

fn rig(tag: &str) -> Rig {
    Rig::new(
        tag,
        FakeProcs::new(),
        FakeApps::new().running(APP_BUNDLE_ID),
    )
}

/// The saved copy as the store would read it back, split or not.
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
        let rig = rig(&format!("big-{size}"));
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
        // And back: the first account's split copy is a good login to switch to.
        rig.store.switch_to("user-1:ws-1").unwrap();
        assert_eq!(std::fs::read(rig.auth()).unwrap(), a);
    }
}

#[test]
fn a_live_login_that_cannot_be_saved_is_never_overwritten() {
    let rig = rig("unsaved");
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
    let rig = rig("anon");
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
    let rig = rig("damaged");
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
    // The Swift app put a long value's hex on argv: one item, read back as it is.
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
