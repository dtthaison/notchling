use std::sync::Arc;

use notchling_local_api::ClientError;
use serde_json::{json, Value};

use super::*;
use crate::actions::Transport;
use crate::inbox::consent_card::{ConsentCard, Env};

const SIGN_IN: &str = "https://claude.ai/oauth/authorize?code=FAKE";

type Calls = Arc<Mutex<Vec<(String, Value)>>>;

struct Fake {
    calls: Calls,
    reply: Value,
}

impl Transport for Fake {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((op.to_owned(), args));
        Ok(self.reply.clone())
    }
}

fn feed_with(reply: Value, calls: &Calls) -> InboxFeed {
    let mut feed = InboxFeed::none();
    feed.transport = Some(Arc::new(Fake {
        calls: calls.clone(),
        reply,
    }));
    feed
}

fn apply(op: &str, args: Value, action: &str, tool: &str) -> AccountApply {
    AccountApply {
        op: op.into(),
        args,
        action: action.into(),
        tool: tool.into(),
    }
}

#[test]
fn an_add_card_opens_the_sign_in_url_from_the_reply_and_keeps_it_nowhere() {
    let calls = Calls::default();
    let feed = feed_with(json!({"login": "signin-1", "url": SIGN_IN}), &calls);
    let add = apply("accounts.add.begin", json!({}), "add", "claude");
    let sign = feed.allow_account("c11", &add).expect("allowed");
    assert!(matches!(&sign, SignIn::OpenUrl(url) if url == SIGN_IN));
    assert_eq!(
        calls.lock().unwrap().clone(),
        vec![("accounts.add.begin".to_owned(), json!({"consentId": "c11"}))]
    );
    assert!(!format!("{sign:?}").contains("FAKE"));
    assert!(!format!("{:?}", calls.lock().unwrap()).contains("FAKE"));
    assert!(feed.snapshot(0).items().is_empty());
    let card = ConsentCard::build(
        &json!({"id": "c11", "kind": "account", "state": "waiting", "expiresMs": 9_999_999_999_999i64,
                "appliedBy": "app",
                "change": {"action": "add", "tool": "claude", "apply": {"op": "accounts.add.begin", "args": {}}}}),
        &Env::default(),
    )
    .expect("card");
    assert!(!format!("{card:?}").contains("FAKE"));
}

#[test]
fn a_provider_add_gives_its_sign_in_url_and_never_a_plain_http_one() {
    let calls = Calls::default();
    let add = apply(
        "providers.add.begin",
        json!({"provider": "codex", "confirm": true}),
        "add",
        "codex",
    );
    let feed = feed_with(
        json!({"state": "waiting", "provider": "codex", "sign_in_url": "https://auth.openai.com/FAKE"}),
        &calls,
    );
    assert!(matches!(
        feed.allow_account("c11", &add),
        Ok(SignIn::OpenUrl(url)) if url == "https://auth.openai.com/FAKE"
    ));
    assert_eq!(
        calls.lock().unwrap()[0].1,
        json!({"provider": "codex", "confirm": true, "consentId": "c11"})
    );
    let plain = feed_with(
        json!({"state": "waiting", "sign_in_url": "http://evil/FAKE"}),
        &calls,
    );
    assert_eq!(plain.allow_account("c11", &add), Ok(SignIn::Nothing));
    let empty = feed_with(json!({"state": "waiting", "sign_in_url": ""}), &calls);
    assert_eq!(empty.allow_account("c11", &add), Ok(SignIn::Nothing));
}

#[test]
fn a_provider_add_still_starting_opens_the_accounts_window() {
    let calls = Calls::default();
    let feed = feed_with(json!({"state": "starting"}), &calls);
    for (tool, provider) in [("codex", "codex"), ("antigravity", "antigravity")] {
        let add = apply(
            "providers.add.begin",
            json!({"provider": provider, "confirm": true}),
            "add",
            tool,
        );
        assert_eq!(
            feed.allow_account("c11", &add),
            Ok(SignIn::OpenAccounts(Some(provider.to_owned())))
        );
    }
    let claude = apply("accounts.add.begin", json!({}), "add", "claude");
    assert_eq!(feed.allow_account("c11", &claude), Ok(SignIn::Nothing));
}

#[test]
fn any_other_reply_opens_nothing() {
    let calls = Calls::default();
    let slow = feed_with(
        json!({"state": "working", "ticket": 3, "op": "accounts.switch"}),
        &calls,
    );
    let switch = apply(
        "accounts.switch",
        json!({"id": "acct-2222222222222222"}),
        "switch",
        "claude",
    );
    assert_eq!(slow.allow_account("c10", &switch), Ok(SignIn::Nothing));
    let leaky = feed_with(json!({"url": SIGN_IN}), &calls);
    assert_eq!(leaky.allow_account("c10", &switch), Ok(SignIn::Nothing));
    let done = feed_with(json!({}), &calls);
    assert_eq!(done.allow_account("c10", &switch), Ok(SignIn::Nothing));
}

#[test]
fn a_refused_allow_is_the_error_the_daemon_gave() {
    let feed = InboxFeed::none();
    let switch = apply("accounts.switch", json!({}), "switch", "claude");
    assert!(feed.allow_account("c10", &switch).is_err());
}
