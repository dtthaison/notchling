use super::*;
use crate::settings::keys::{spec, KEYS};

#[test]
fn every_key_has_a_class_a_title_a_description_and_a_reason() {
    let missing: Vec<&str> = KEYS
        .iter()
        .filter(|k| k.access == Access::Unset)
        .map(|k| k.name)
        .collect();
    assert!(
        missing.is_empty(),
        "these keys have no agent class: add .free(..), .ask(..) or .never(..) to each in keys.rs: {missing:?}"
    );
    for k in KEYS {
        assert!(!k.title.trim().is_empty(), "{} has no title", k.name);
        assert!(!k.about.trim().is_empty(), "{} has no description", k.name);
        assert!(
            !k.why.trim().is_empty(),
            "{} has no reason for its class",
            k.name
        );
    }
    for s in SWITCHES {
        assert!(!s.title.is_empty() && !s.about.is_empty(), "{}", s.name);
        assert!(spec(s.key).is_none(), "{} is a switch, not a key", s.key);
        assert_eq!(s.key, format!("switch.{}", s.name));
    }
}

#[test]
fn switches_logins_and_sandbox_permissions_are_never_an_agents() {
    for name in [
        "bridgeEnabled",
        "codexNetwork",
        "accounts",
        "codexAccounts",
        "antigravityAccounts",
        "claudeDesktopAccounts",
        "debugAccessCertsFile",
        "vaultReminderSent",
        "newSessionChoice",
    ] {
        assert_eq!(spec(name).map(|k| k.access), Some(Access::Never), "{name}");
    }
    let names: Vec<&str> = SWITCHES.iter().map(|s| s.name).collect();
    assert_eq!(names, ["acting", "bridge", "accounts", "vault", "install"]);
    assert_eq!(switch("vault").map(|s| s.title), Some("Secret Vault"));
    assert_eq!(switch("switch.vault").map(|s| s.name), Some("vault"));
    assert_eq!(switch("switch.accounts").map(|s| s.name), Some("accounts"));
    assert!(
        switch("accounts").is_none(),
        "the saved accounts key, not the switch"
    );
    assert!(switch("chatHistory").is_none());
}

#[test]
fn only_plain_values_are_free_or_asked() {
    for k in KEYS.iter().filter(|k| k.access.shown()) {
        assert!(
            matches!(k.kind, Kind::Bool | Kind::Int | Kind::Double | Kind::String),
            "{} is {:?}: an agent can't sensibly write it",
            k.name,
            k.kind
        );
    }
    assert_eq!(spec("chatHistory").map(|k| k.access), Some(Access::Free));
    assert_eq!(spec("limitMode").map(|k| k.access), Some(Access::Ask));
    assert_eq!(Access::Unset.as_str(), "never");
    assert!(!Access::Unset.shown() && !Access::Never.shown());
}

#[test]
fn text_from_the_command_line_becomes_the_keys_type() {
    let get = |name: &str| spec(name).unwrap_or_else(|| panic!("{name}"));
    assert_eq!(get("showPet").from_text("Off"), json!(false));
    assert_eq!(get("showPet").from_text("true"), json!(true));
    assert_eq!(get("showPet").from_text("maybe"), json!("maybe"));
    assert_eq!(get("limitSwitchAt").from_text(" 90 "), json!(90));
    assert_eq!(get("chatPageWidth").from_text("600.5"), json!(600.5));
    assert_eq!(get("chatPageWidth").from_text("NaN"), json!("NaN"));
    assert_eq!(get("chatHistory").from_text("8MB"), json!("8mb"));
    assert_eq!(get("mode").from_text("whileworking"), json!("whileWorking"));
    assert_eq!(get("notchWidth").from_text("huge"), json!("huge"));
    assert_eq!(
        get("chatProjectFolders").from_text("[\"/a\"]"),
        json!(["/a"])
    );
    assert_eq!(get("showPet").read_value(json!(true)), json!(true));
    assert_eq!(get("showPet").read_value(json!("no")), json!(false));
}

#[test]
fn a_value_is_checked_against_type_choices_and_range() {
    let get = |name: &str| spec(name).unwrap_or_else(|| panic!("{name}"));
    assert_eq!(get("chatHistory").check(&json!("16mb")), Ok(()));
    assert_eq!(
        get("chatHistory").check(&json!("1gb")),
        Err("chatHistory takes one of 2mb, 4mb, 8mb, 16mb".into())
    );
    assert_eq!(
        get("showPet").check(&json!("yes please")),
        Err("showPet takes true or false".into())
    );
    assert_eq!(get("limitSwitchAt").check(&json!(85)), Ok(()));
    assert_eq!(
        get("limitSwitchAt").check(&json!(100)),
        Err("limitSwitchAt takes a whole number from 80 to 99".into())
    );
    assert!(get("limitSwitchAt").check(&json!(85.5)).is_err());
    assert_eq!(get("chatPageWidth").check(&json!(480)), Ok(()));
    assert!(get("chatPageWidth").check(&json!("wide")).is_err());
    assert_eq!(get("chatPageWidth").kind_name(), "number");
    assert_eq!(get("showPet").kind_name(), "bool");
    assert_eq!(get("dismissed").kind_name(), "object");
}
