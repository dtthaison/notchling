use super::tests::{agent, app, ask, ended, err, seed, setup};
use super::*;
use notchling_events::field::Hex;
use notchling_events::scope::{self, Scope};

fn save_with(s: &super::tests::Setup, event: &Value, extra: Value) -> Result<Value, ApiError> {
    let mut args = json!({"entry": event["entry"], "value": {"secret": "fake-npm-0007"},
                          "consentId": event["id"]});
    for (key, value) in extra.as_object().cloned().unwrap_or_default() {
        args[key] = value;
    }
    s.ops.call(&app(), ops::VAULT_SAVE, &args)
}

#[test]
fn a_save_is_refused_when_the_secret_appeared_after_the_card_was_shown() {
    let s = setup("consent-appeared", true);
    let (_, event) = ask(&s, json!({"name": "npm"}));
    s.ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": {"name": "npm", "kind": "token"}, "value": {"secret": "fake-npm-0001"}}),
        )
        .unwrap();
    let e = err(save_with(&s, &event, json!({})));
    assert_eq!((e.code, e.data.unwrap()["reason"].clone()), (ErrorCode::Forbidden, json!("needs_consent")));
    let id = event["id"].as_str().unwrap();
    assert_eq!(ended(&s, id)["state"], "withdrawn");
}

#[test]
fn a_save_is_refused_when_the_secret_it_would_replace_went_away() {
    let s = setup("consent-gone", true);
    seed(&s, false);
    let (_, event) = ask(&s, json!({"name": "gh", "note": "new"}));
    assert_eq!(event["existing"], true);
    s.ops
        .call(&app(), ops::VAULT_REMOVE, &json!({"name": "gh"}))
        .unwrap();
    let e = err(save_with(&s, &event, json!({"replacing": "gh"})));
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert_eq!(ended(&s, event["id"].as_str().unwrap())["state"], "withdrawn");
}

#[test]
fn a_save_that_replaces_another_name_is_refused() {
    let s = setup("consent-replacing", true);
    seed(&s, false);
    let (_, event) = ask(&s, json!({"name": "npm"}));
    let e = err(save_with(&s, &event, json!({"replacing": "gh"})));
    assert_eq!(e.code, ErrorCode::Forbidden);
    let kept = s
        .ops
        .call(&app(), ops::VAULT_LIST, &json!({}))
        .unwrap()
        .to_string();
    assert!(kept.contains("\"gh\"") && !kept.contains("\"npm\""), "{kept}");
    assert_eq!(ended(&s, event["id"].as_str().unwrap())["state"], "withdrawn");
}

#[test]
fn an_agent_never_reads_the_entry_a_request_found_or_the_changes_it_would_make() {
    let s = setup("consent-redact", true);
    seed(&s, false);
    let (reply, event) = ask(&s, json!({"name": "gh", "url": "https://api.github-cdn.io"}));
    assert_eq!(event["existing"], true);
    let shown = reply["request"].to_string();
    for hidden in ["api.github.com", "GH_TOKEN", "hostChange", "\"existing\"", "revision"] {
        assert!(!shown.contains(hidden), "{hidden} in {shown}");
    }
    let id = event["id"].as_str().unwrap();
    let read = s
        .ops
        .call(&agent(), ops::VAULT_REQUEST_CANCEL, &json!({"id": id}))
        .unwrap_err();
    assert_eq!(read.code, ErrorCode::Forbidden);
}

#[test]
fn while_the_vault_is_off_a_request_reads_nothing_and_the_card_is_built_live_when_it_turns_on() {
    let s = setup("consent-live", true);
    seed(&s, false);
    s.switch.set(false);
    *s.item.reads.lock().unwrap() = 0;
    let (reply, event) = ask(&s, json!({"name": "gh", "note": "later"}));
    assert!(event.is_null());
    assert_eq!(*s.item.reads.lock().unwrap(), 0, "no store read while off");
    let stored = s
        .consent
        .store()
        .get(reply["request"]["id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert!(stored.change["existing"].is_null(), "{}", stored.change);
    s.switch.set(true);
    let shown = s.room.pending(topics::VAULT_REQUEST_PENDING).unwrap();
    assert_eq!(shown["existing"], true, "{shown}");
    assert_eq!(shown["entry"]["note"], "later");
}

#[test]
fn the_op_id_of_the_save_is_recorded_on_the_request() {
    let s = setup("consent-op", true);
    let (_, event) = ask(&s, json!({"name": "npm"}));
    let _scope = scope::enter(Scope {
        op_id: Hex::new("abcdef0123456789"),
        actor: None,
    });
    save_with(&s, &event, json!({})).unwrap();
    let row = s
        .consent
        .store()
        .get(event["id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(row.used_op_id.as_deref(), Some("abcdef0123456789"));
    assert_eq!(
        scope::take_consent().map(|(id, _)| id),
        Some(event["id"].as_str().unwrap().to_string())
    );
}
