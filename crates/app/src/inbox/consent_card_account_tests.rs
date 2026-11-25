use serde_json::{json, Value};

use super::*;

const NOW: i64 = 1_794_096_000_000;

fn env() -> Env {
    Env {
        now_ms: NOW,
        vault_on: true,
        shown_ms: 5_000,
        node: None,
        ..Env::default()
    }
}

fn account(change: Value, facts: Value) -> Value {
    json!({
        "id": "c10", "kind": "account", "target": "switch:acct-2222222222222222", "state": "waiting",
        "change": change, "facts": facts,
        "tool": "claude", "verified": true, "session": "0b1c2d3e4f5a",
        "expiresMs": NOW + 14 * 60_000, "appliedBy": "app", "answerOn": "mac"
    })
}

fn switch_change() -> Value {
    json!({
        "action": "switch", "tool": "claude", "organization": "acme",
        "account": "acct-2222222222222222", "before": "acct-1111111111111111",
        "args": {"account": "acct-2222222222222222"},
        "apply": {"op": "accounts.switch", "args": {"id": "acct-2222222222222222"}}
    })
}

fn facts_of(lines: Value) -> Vec<String> {
    let card = ConsentCard::build(&account(switch_change(), lines), &env()).expect("card");
    card.facts
}

#[test]
fn each_account_fact_line_renders_its_english_text() {
    let cases: Vec<(Value, String)> = vec![
        (
            json!({"line": "consent-account",
                   "args": {"tool": "claude", "account": "b***@w***.com", "organization": "acme"}}),
            "The Claude Code account b***@w***.com in acme".into(),
        ),
        (
            json!({"line": "consent-account-switch",
                   "args": {"tool": "claude", "account": "b***@w***.com", "before": "a***@w***.com"}}),
            "New Claude Code sessions use b***@w***.com instead of a***@w***.com".into(),
        ),
        (
            json!({"line": "consent-account-switch-first",
                   "args": {"tool": "codex", "account": "d***@w***.com"}}),
            "New Codex sessions use d***@w***.com".into(),
        ),
        (
            json!({"line": "consent-account-switch-codex", "args": {}}),
            "If the ChatGPT app or Codex's background server is running, it restarts and its running tasks stop. Open Codex sessions keep the old account until they start again".into(),
        ),
        (
            json!({"line": "consent-account-switch-antigravity", "args": {}}),
            "If Antigravity is running, it quits and opens again on the other account. Agents running in it stop".into(),
        ),
        (
            json!({"line": "consent-account-add", "args": {"tool": "codex", "organization": "acme"}}),
            "Sign in to a new Codex account in your browser. It joins acme".into(),
        ),
        (
            json!({"line": "consent-account-remove",
                   "args": {"tool": "antigravity", "account": "b***@w***.com"}}),
            "Remove the Antigravity account b***@w***.com and its saved sign-in from Notchling"
                .into(),
        ),
        (
            json!({"line": "consent-account-move-all", "args": {"count": 2, "account": "c***@w***.com"}}),
            "Sessions moved from lanes to the main login, signed in as c***@w***.com: 2".into(),
        ),
        (
            json!({"line": "consent-account-signed-out", "args": {"count": 1}}),
            "Sessions in that lane outside Notchling that are signed out: 1".into(),
        ),
        (
            json!({"line": "consent-account-move-session",
                   "args": {"name": "notchling", "folder": "~/go/src/pp/notchling", "account": "c***@w***.com"}}),
            "Move notchling (~/go/src/pp/notchling) to the login of c***@w***.com. It stops and resumes there".into(),
        ),
        (
            json!({"line": "consent-lane-create", "args": {"account": "b***@w***.com"}}),
            "Open a lane for b***@w***.com, so sessions can run on it beside the main login"
                .into(),
        ),
        (
            json!({"line": "consent-lane-close", "args": {"account": "c***@w***.com"}}),
            "Close the lane of c***@w***.com".into(),
        ),
        (
            json!({"line": "consent-lane-switch",
                   "args": {"before": "c***@w***.com", "account": "b***@w***.com"}}),
            "The lane of c***@w***.com signs in as b***@w***.com instead".into(),
        ),
        (
            json!({"line": "consent-lane-sessions", "args": {"count": 0}}),
            "Sessions running in that lane now: 0".into(),
        ),
    ];
    for (line, english) in cases {
        assert_eq!(facts_of(json!([line.clone()])), vec![english], "{line}");
    }
}

#[test]
fn an_unknown_fact_line_is_skipped() {
    let facts = facts_of(json!([
        {"line": "consent-account-teleport", "args": {"tool": "claude"}},
        {"line": "consent-lane-close", "args": {"account": "c***@w***.com"}},
        {"line": "consent-account-add", "args": {"organization": "acme"}},
        {"line": "consent-lane-sessions", "args": {}},
    ]));
    assert_eq!(facts, vec!["Close the lane of c***@w***.com".to_owned()]);
}

#[test]
fn an_account_card_allows_by_calling_its_apply_op_with_the_consent_id() {
    let view = account(switch_change(), json!([]));
    let card = ConsentCard::build(&view, &env()).expect("card");
    let apply = card.account_apply.clone().expect("apply");
    assert_eq!(
        apply,
        AccountApply {
            op: "accounts.switch".into(),
            args: json!({"id": "acct-2222222222222222"}),
            action: "switch".into(),
            tool: "claude".into(),
        }
    );
    assert_eq!(
        crate::inbox::model::cluster_apply_args("c10", &apply.args),
        json!({"id": "acct-2222222222222222", "consentId": "c10"})
    );
    assert!(card.app_applied);
    assert_eq!(card.cluster_apply, None);
    assert_eq!(card.switch, None);
}

#[test]
fn only_an_account_request_carries_an_account_apply() {
    let mut view = account(switch_change(), json!([]));
    view["kind"] = json!("cluster");
    let card = ConsentCard::build(&view, &env()).expect("card");
    assert_eq!(card.account_apply, None);
    assert!(card.cluster_apply.is_some());
    let bare = account(json!({"action": "switch"}), json!([]));
    assert_eq!(
        ConsentCard::build(&bare, &env())
            .expect("card")
            .account_apply,
        None
    );
    let mut no_tool = switch_change();
    no_tool.as_object_mut().unwrap().remove("tool");
    assert_eq!(
        ConsentCard::build(&account(no_tool, json!([])), &env())
            .expect("card")
            .account_apply,
        None
    );
}

#[test]
fn the_card_shows_the_organization_and_masked_email_only() {
    let facts = json!([
        {"line": "consent-account",
         "args": {"tool": "claude", "account": "b***@w***.com", "organization": "acme"}},
        {"line": "consent-account-switch",
         "args": {"tool": "claude", "account": "b***@w***.com", "before": "a***@w***.com"}},
    ]);
    let card = ConsentCard::build(&account(switch_change(), facts), &env()).expect("card");
    assert!(card
        .details
        .iter()
        .any(|d| d.label == t::consent::card_label_organization() && d.value == "acme"));
    let shown = format!("{:?} {:?}", card.facts, card.details);
    for secret in ["bob@", "ann@", "acct-", "a1a1", "lane-", "https://"] {
        assert!(!shown.contains(secret), "{secret} in {shown}");
    }
    assert!(shown.contains("b***@w***.com"));
}
