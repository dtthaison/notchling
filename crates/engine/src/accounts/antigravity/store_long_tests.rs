use super::login::{encode_keyring, fake_login};
use super::store::{LIVE_ACCOUNT, LIVE_SERVICE, SERVICE};
use super::store_tests::Rig;
use crate::accounts::chunked::{self, part_name, Plain};
use crate::accounts::procs::FakeApps;
use crate::providers::AccountProvider;

fn big_login(sub: &str, email: &str) -> String {
    let filler: String = "FAKE-access-".chars().cycle().take(2_400).collect();
    fake_login(sub, email, &filler)
}

fn saved_copy(rig: &Rig, id: &str) -> Option<String> {
    chunked::read(&Plain(rig.keychain.as_ref()), SERVICE, id)
        .ok()
        .flatten()
}

fn no_quit(rig: &Rig) -> bool {
    !rig.apps.events().iter().any(|e| e.starts_with("quit"))
}

#[test]
fn a_long_live_login_is_saved_in_parts_before_the_switch() {
    let rig = Rig::new("long-long-live", FakeApps::new());
    let a = big_login("1", "a@example.com");
    rig.keychain
        .set_as_other_tool(LIVE_SERVICE, LIVE_ACCOUNT, &encode_keyring(&a));
    let b = fake_login("2", "b@example.com", "tb");
    rig.save("2", &b);

    rig.store.switch_to("2").unwrap();

    assert_eq!(rig.live().as_deref(), Some(b.as_str()));
    assert_eq!(saved_copy(&rig, "1").as_deref(), Some(a.as_str()));
    assert!(rig.keychain.get(SERVICE, &part_name("1", 0)).is_some());
}

#[test]
fn a_login_that_cannot_be_saved_is_never_replaced() {
    let rig = Rig::new(
        "long-unsaved",
        FakeApps::new().running("com.google.antigravity"),
    );
    let a = fake_login("1", "a@example.com", "ta");
    rig.set_live(&a);
    rig.save("2", &fake_login("2", "b@example.com", "tb"));
    rig.keychain.fail_writes();

    let err = rig.store.switch_to("2").unwrap_err();

    assert!(err.message().contains("Couldn't save"), "{}", err.message());
    assert_eq!(rig.live().as_deref(), Some(a.as_str()));
    assert!(no_quit(&rig), "Antigravity was left running");
}

#[test]
fn a_saved_login_too_long_for_antigravitys_item_is_refused_before_anything_quits() {
    let rig = Rig::new(
        "long-too-long",
        FakeApps::new().running("com.google.antigravity"),
    );
    let a = fake_login("1", "a@example.com", "ta");
    rig.set_live(&a);
    let long = big_login("2", "b@example.com");
    chunked::write(&Plain(rig.keychain.as_ref()), SERVICE, "2", &long).unwrap();

    let err = rig.store.switch_to("2").unwrap_err();

    assert!(err.message().contains("too long"), "{}", err.message());
    assert_eq!(rig.live().as_deref(), Some(a.as_str()));
    assert!(no_quit(&rig));
}

#[test]
fn an_add_that_could_not_put_a_long_login_back_is_refused() {
    let rig = Rig::new("long-add-long", FakeApps::new());
    let a = big_login("1", "a@example.com");
    rig.keychain
        .set_as_other_tool(LIVE_SERVICE, LIVE_ACCOUNT, &encode_keyring(&a));

    let err = rig.store.begin_add().unwrap_err();

    assert!(err.message().contains("too long"), "{}", err.message());
    assert_eq!(rig.live().as_deref(), Some(a.as_str()), "still signed in");
    assert!(no_quit(&rig));
}

#[test]
fn an_add_abandoned_when_accounts_turn_off_writes_nothing_and_says_where_it_left_things() {
    let rig = Rig::new("long-abandon", FakeApps::new());
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    rig.store.begin_add().unwrap();
    let calls = rig.keychain.calls().len();
    let note = rig.store.abandon_add().unwrap_or_default();
    assert!(note.contains("sign-in screen"), "{note}");
    assert_eq!(rig.keychain.calls().len(), calls, "no Keychain call");
    assert!(!rig.store.state().adding);
    rig.store.cancel_add();
    assert_eq!(rig.keychain.calls().len(), calls);
}
