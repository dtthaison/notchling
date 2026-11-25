use super::tests::{agent, app, setup};
use super::*;

fn refused(s: &super::tests::Setup, args: Value) -> ApiError {
    s.ops
        .call(&agent(), ops::VAULT_REQUEST, &args)
        .expect_err("the request is refused")
}

#[test]
fn a_generic_name_is_refused_with_the_naming_rule_even_without_the_cli() {
    let s = setup("naming-generic", true);
    for name in ["api", "dashboards", "login", "admin-password"] {
        let e = refused(&s, json!({"name": name, "kind": "login"}));
        assert!(e.message.contains("doesn't say which service"), "{name}");
        assert!(
            e.message.contains("emqx-dashboard-dev-admin"),
            "{name}: {}",
            e.message
        );
    }
}

fn dry(s: &super::tests::Setup, name: &str) -> Value {
    s.ops
        .call(
            &agent(),
            ops::VAULT_REQUEST,
            &json!({"name": name, "dryRun": true}),
        )
        .unwrap()
}

fn save_login(s: &super::tests::Setup, name: &str) {
    s.ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": {"name": name, "kind": "login"},
                    "value": {"username": "fake-user", "secret": "fake-pass"}}),
        )
        .unwrap();
}

#[test]
fn a_generic_name_is_not_refused_while_the_vault_is_off_because_the_daemon_cannot_tell() {
    let s = setup("naming-off", false);
    let reply = s
        .ops
        .call(&agent(), ops::VAULT_REQUEST, &json!({"name": "api"}))
        .unwrap();
    assert_eq!(reply["state"], "waiting");
}

#[test]
fn a_dry_run_says_the_same_about_a_generic_name_whether_or_not_it_is_saved() {
    let s = setup("naming-dry", true);
    save_login(&s, "login");
    let saved = dry(&s, "login");
    let unsaved = dry(&s, "api");
    assert_eq!(saved["would"], "ask");
    assert_eq!(unsaved["would"], saved["would"]);
    assert_eq!(unsaved["error"], saved["error"]);
}

#[test]
fn an_unreadable_vault_index_is_a_failure_not_a_naming_refusal() {
    let s = setup("naming-unreadable", true);
    std::fs::create_dir_all(s.dir.join("daemon")).unwrap();
    std::fs::write(s.dir.join("daemon/vault.json"), "{not json").unwrap();
    let e = refused(&s, json!({"name": "login", "kind": "login"}));
    assert!(
        !e.message.contains("doesn't say which service"),
        "{}",
        e.message
    );
}

#[test]
fn a_secret_the_user_already_saved_under_a_generic_name_can_still_be_renewed() {
    let s = setup("naming-existing", true);
    save_login(&s, "login");
    let reply = s
        .ops
        .call(
            &agent(),
            ops::VAULT_REQUEST,
            &json!({"name": "login", "kind": "login"}),
        )
        .unwrap();
    assert_eq!(reply["state"], "waiting");
    let e = refused(&s, json!({"name": "api"}));
    assert!(e.message.contains("doesn't say which service"));
}

#[test]
fn a_name_with_a_service_part_is_asked_as_before() {
    let s = setup("naming-ok", true);
    let reply = s
        .ops
        .call(
            &agent(),
            ops::VAULT_REQUEST,
            &json!({"name": "emqx-dashboard-dev-admin"}),
        )
        .unwrap();
    assert_eq!(reply["state"], "waiting");
}
