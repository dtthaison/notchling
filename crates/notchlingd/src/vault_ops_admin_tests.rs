use super::tests::{agent, app, seed, setup};
use super::*;
use crate::event_assert::{of, results, text};

const PASSWORD: &str = "fake-password-0002";

fn admin(name: &str) -> Value {
    json!({"name": name, "machine": "m2"})
}

#[test]
fn the_app_reads_a_password_for_a_machine_and_the_audit_names_only_the_entry() {
    let s = setup("admin-read", true);
    seed(&s, false);
    let reply = s.ops.call(&app(), ops::VAULT_ADMIN, &admin("REG")).unwrap();
    assert_eq!(reply["secret"], PASSWORD);
    let used = of(&s.sink, "vault.secret_used");
    assert_eq!(used.len(), 1);
    assert_eq!(text(&used[0], "result").as_deref(), Some("given"));
    let audit = format!("{:?}", s.sink.events());
    assert!(audit.contains("reg"), "{audit}");
    assert!(
        audit.contains("sudo") && !audit.contains(PASSWORD),
        "{audit}"
    );
    let outside = format!("{:?}", s.room.events.lock().unwrap());
    assert!(!outside.contains(PASSWORD));
}

#[test]
fn only_the_app_may_ask_and_the_vault_must_be_on() {
    let s = setup("admin-who", true);
    seed(&s, false);
    let e = s
        .ops
        .call(&agent(), ops::VAULT_ADMIN, &admin("reg"))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert!(!e.message.contains(PASSWORD));
    assert!(of(&s.sink, "vault.secret_used").is_empty());
    assert_eq!(s.ops.spec(ops::VAULT_ADMIN), Some(OpSpec::APP));
    s.switch.set(false);
    let e = s
        .ops
        .call(&app(), ops::VAULT_ADMIN, &admin("reg"))
        .unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::Forbidden, VAULT_OFF)
    );
}

#[test]
fn a_missing_entry_is_said_by_name_and_bad_arguments_are_refused() {
    let s = setup("admin-missing", true);
    seed(&s, false);
    let e = s
        .ops
        .call(&app(), ops::VAULT_ADMIN, &admin("nope"))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "nope isn't in the Vault.");
    for bad in [
        json!({}),
        json!({"name": "reg"}),
        json!({"name": 3, "machine": "m"}),
    ] {
        assert_eq!(
            s.ops.call(&app(), ops::VAULT_ADMIN, &bad).unwrap_err().code,
            ErrorCode::BadRequest
        );
    }
}

#[test]
fn an_ask_first_entry_waits_for_the_user_and_with_no_app_is_not_read() {
    let s = setup("admin-ask", true);
    seed(&s, false);
    let ask = json!({"name": "reg", "kind": "login", "ask": true});
    s.ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": ask, "replacing": "reg", "value": {"username": "u", "secret": PASSWORD}}),
        )
        .unwrap();
    s.ops.store.forget();
    let reads = *s.item.reads.lock().unwrap();
    let e = s
        .ops
        .call(&app(), ops::VAULT_ADMIN, &admin("reg"))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsApproval);
    assert_eq!(*s.item.reads.lock().unwrap(), reads, "no value read");
    assert_eq!(results(&s.sink, "vault.secret_used"), ["no_app"]);
}
