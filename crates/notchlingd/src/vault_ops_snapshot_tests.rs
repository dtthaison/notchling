use super::tests::{agent, app, ask, ended, err, seed, setup, setup_full};
use super::*;

fn save_for(s: &super::tests::Setup, id: &str, name: &str, extra: Value) -> Result<Value, ApiError> {
    let mut args = json!({"entry": {"name": name, "kind": "token"},
                          "value": {"secret": "fake-gh-0009"}, "consentId": id});
    for (key, value) in extra.as_object().cloned().unwrap_or_default() {
        args[key] = value;
    }
    s.ops.call(&app(), ops::VAULT_SAVE, &args)
}

fn row(s: &super::tests::Setup, id: &str) -> notchling_engine::consent::Consent {
    s.consent.store().get(id).unwrap().unwrap()
}

#[test]
fn a_request_opened_while_the_vault_was_off_keeps_the_entry_shown_once_it_turns_on() {
    let s = setup("snap-off-on", true);
    seed(&s, false);
    s.switch.set(false);
    let (reply, _) = ask(&s, json!({"name": "gh", "note": "later"}));
    let id = reply["request"]["id"].as_str().unwrap().to_string();
    assert!(row(&s, &id).change["existing"].is_null());
    s.switch.set(true);
    let shown = s.room.pending(topics::VAULT_REQUEST_PENDING).unwrap();
    assert_eq!(shown["existing"], true, "{shown}");
    let kept = row(&s, &id);
    assert_eq!(kept.change["existing"], true, "{}", kept.change);
    let saved = s.ops.store.entry("gh").unwrap().unwrap();
    assert_eq!(kept.change["revision"], json!(saved.updated));
    s.ops
        .call(&app(), ops::VAULT_REMOVE, &json!({"name": "gh"}))
        .unwrap();
    let e = err(save_for(&s, &id, "gh", json!({"replacing": "gh"})));
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert_eq!(ended(&s, &id)["state"], "withdrawn");
}

#[test]
fn a_save_is_refused_while_the_entry_the_card_shows_was_never_read() {
    let s = setup("snap-unread", true);
    s.switch.set(false);
    let (reply, _) = ask(&s, json!({"name": "npm"}));
    let id = reply["request"]["id"].as_str().unwrap().to_string();
    let index = s.dir.join("daemon").join("vault.json");
    std::fs::create_dir_all(index.parent().unwrap()).unwrap();
    std::fs::write(&index, "not json").unwrap();
    s.switch.set(true);
    assert!(row(&s, &id).change["existing"].is_null());
    std::fs::remove_file(&index).unwrap();
    let e = err(save_for(&s, &id, "npm", json!({})));
    assert_eq!(
        (e.code, e.data.unwrap()["reason"].clone()),
        (ErrorCode::Forbidden, json!("needs_consent"))
    );
    assert!(s.ops.store.entry("npm").unwrap().is_none());
}

#[test]
fn a_change_under_the_save_lock_is_caught_by_the_store() {
    let s = setup("snap-lock", true);
    let entry = VaultEntry::from_request(&notchling_engine::vault::VaultRequest {
        name: "npm".into(),
        kind: None,
        url: None,
        note: None,
        how_to: None,
        expires: None,
        reason: None,
    });
    let value = notchling_engine::vault::VaultValue {
        secret: "fake-npm-0003".into(),
        ..Default::default()
    };
    let refused = s
        .ops
        .store
        .save_checked(&entry, Some(value), None, "2026-11-21T00:00:00Z", &|_| false);
    assert!(matches!(
        refused,
        Err(notchling_engine::vault::VaultError::Changed)
    ));
    assert!(s.ops.store.entry("npm").unwrap().is_none());
}

#[test]
fn neither_a_dry_run_nor_the_reply_tells_an_agent_whether_a_secret_exists() {
    let s = setup("snap-oracle", true);
    seed(&s, false);
    let dry = |name: &str| {
        let mut reply = s
            .ops
            .call(
                &agent(),
                ops::VAULT_REQUEST,
                &json!({"name": name, "dryRun": true}),
            )
            .unwrap();
        reply["change"]["name"] = json!("x");
        reply["request"]["facts"][0]["args"]["name"] = json!("x");
        reply
    };
    assert_eq!(dry("gh"), dry("npm"));
    let (reply, _) = ask(&s, json!({"name": "gh"}));
    let facts = reply["request"]["facts"].to_string();
    assert!(!facts.contains("update"), "{facts}");
    let id = reply["request"]["id"].as_str().unwrap();
    let seen = s.consent.project(&row(&s, id), true);
    assert!(!seen["facts"].to_string().contains("update"), "{seen}");
    let card = s.consent.project(&row(&s, id), false);
    assert_eq!(card["facts"][0]["args"]["update"], true, "{card}");
}

#[test]
fn the_card_and_the_row_say_the_same_about_who_asks() {
    let trusting: TrustedSession = Arc::new(|_| Some("sess-9".to_string()));
    let s = setup_full("snap-who", true, None, None, Some(trusting));
    let (reply, event) = ask(&s, json!({"name": "npm"}));
    let id = reply["request"]["id"].as_str().unwrap();
    let stored = row(&s, id);
    assert!(stored.verified);
    assert_eq!(event["verified"]["verified"], json!(true), "{event}");
    assert_eq!(stored.change["verified"], event["verified"]);
    assert_eq!(event["session"], "sess-9");
}

#[test]
fn a_retry_of_a_closed_request_shows_no_card_again() {
    let s = setup("snap-retry", true);
    let args = json!({"name": "npm", "consent": {"key": "k-1"}});
    let (reply, _) = ask(&s, args.clone());
    let id = reply["request"]["id"].as_str().unwrap().to_string();
    s.ops
        .call(&app(), ops::VAULT_REQUEST_CANCEL, &json!({"id": id}))
        .unwrap();
    ended(&s, &id);
    let shown = || {
        s.room
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.1 == topics::VAULT_REQUEST_PENDING)
            .count()
    };
    let before = shown();
    let again = s.ops.call(&agent(), ops::VAULT_REQUEST, &args).unwrap();
    assert_eq!(again["request"]["id"], json!(id));
    assert_eq!(shown(), before);
}
