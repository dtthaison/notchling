use super::tests::{agent, ask, asked, rig};
use super::*;
use crate::consent_testkit::{shell, verified};
use notchling_local_api::bind_identity;

#[test]
fn another_session_never_sees_a_pending_ask_through_get_or_asked() {
    let r = rig("scope");
    let id = {
        let _own = bind_identity(Some(verified("s-owner")));
        ask(&r, "limitMode", json!("notify"))
    };
    {
        let _own = bind_identity(Some(verified("s-owner")));
        let mine = r
            .call(&agent(), ops::SETTINGS_GET, json!({"key": "limitMode"}))
            .unwrap();
        assert_eq!(mine["pending"][0]["id"], id.as_str());
    }
    for who in [verified("s-other"), shell("Terminal")] {
        let _other = bind_identity(Some(who));
        let theirs = r
            .call(&agent(), ops::SETTINGS_GET, json!({"key": "limitMode"}))
            .unwrap();
        assert!(theirs.get("pending").is_none(), "{theirs}");
        let list = r.call(&agent(), ops::SETTINGS_LIST, json!({})).unwrap();
        assert!(!format!("{list}").contains(&id), "{list}");
        let err = r
            .call(&agent(), ops::SETTINGS_ASKED, json!({"id": id}))
            .unwrap_err();
        assert_eq!(err.data.unwrap()["reason"], "other_session");
    }
    let _own = bind_identity(Some(verified("s-owner")));
    assert_eq!(asked(&r, &id)["state"], "waiting");
}

#[test]
fn asked_answers_only_for_a_setting_request() {
    let r = rig("asked-kind");
    let row = r
        .consent
        .open(
            &agent(),
            crate::consent_ops::Opening {
                parts: None,
                kind: notchling_engine::consent::SECRET_REQUEST,
                target: "npm".into(),
                change: json!({"name": "npm"}),
                facts: json!([]),
                request: json!({"op": "vault.request", "name": "npm"}),
                reason: None,
                expires_in_ms: None,
                key: None,
            },
        )
        .unwrap()
        .row()
        .clone();
    let err = r
        .call(&agent(), ops::SETTINGS_ASKED, json!({"id": row.id}))
        .unwrap_err();
    assert!(err.message.contains("No such ask"), "{}", err.message);
}

#[test]
fn a_retry_that_follows_the_answer_returns_the_first_reply() {
    let r = rig("retry-applied");
    r.app_follows_the_inbox();
    let set = || {
        r.call(
            &agent(),
            ops::SETTINGS_SET,
            json!({"key": "limitMode", "value": "notify", "consent": {"key": "k-1"}}),
        )
        .unwrap()
    };
    let first = set();
    let id = first["id"].as_str().unwrap().to_string();
    r.inbox
        .decide(
            &r.item_id(&id),
            notchling_engine::inbox::ActionId::Allow,
            "mac",
        )
        .unwrap();
    r.wait_for("applied", || asked(&r, &id)["state"] == "applied");
    let again = set();
    assert_eq!(again["id"], first["id"]);
    assert_eq!(again["state"], "applied");
    assert_eq!(r.inbox.items().len(), 1, "no second item");
}
