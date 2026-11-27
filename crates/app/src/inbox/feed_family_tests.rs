use std::sync::{Arc, Mutex};

use notchling_local_api::{ApiError, ClientError, ErrorCode};
use serde_json::{json, Value};

use super::*;
use crate::actions::Transport;
use crate::inbox::consent_card::{ConsentCard, Env};
use crate::inbox::consent_family::{toggled, Admin};
use crate::inbox::family_fixtures::{cost_view, env, install_view, machine_view, update_view};

type Calls = Arc<Mutex<Vec<(String, Value)>>>;

struct Fake {
    calls: Calls,
    reply: Result<Value, ApiError>,
}

impl Transport for Fake {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((op.to_owned(), args));
        self.reply.clone().map_err(ClientError::Api)
    }
}

fn feed_with(reply: Result<Value, ApiError>, calls: &Calls) -> InboxFeed {
    let mut feed = InboxFeed::none();
    feed.transport = Some(Arc::new(Fake {
        calls: calls.clone(),
        reply,
    }));
    feed
}

fn built(view: &Value, unticked: &[&str]) -> ConsentCard {
    let mut set = std::collections::HashSet::new();
    for id in unticked {
        toggled(&mut set, id);
    }
    ConsentCard::build(
        view,
        &Env {
            unticked: set,
            ..env()
        },
    )
    .expect("card")
}

#[test]
fn allow_on_an_update_card_sends_exactly_its_apply_op_with_the_consent_id() {
    let calls = Calls::default();
    let feed = feed_with(Ok(json!({"done": true})), &calls);
    let card = built(&update_view(), &[]);
    let note = feed
        .allow_family(
            "c20",
            card.family_apply.as_ref().expect("apply"),
            &card.parts,
            None,
        )
        .expect("allowed");
    assert_eq!(note, None);
    assert_eq!(
        calls.lock().unwrap().clone(),
        vec![(
            "versions.update_local".to_owned(),
            json!({"tool": "codex", "consentId": "c20"})
        )]
    );
}

#[test]
fn allow_on_an_install_card_sends_only_the_kept_part_ids() {
    let calls = Calls::default();
    let feed = feed_with(Ok(json!({"parts": []})), &calls);
    let card = built(&install_view(), &["hooks"]);
    feed.allow_family(
        "c20",
        card.family_apply.as_ref().expect("apply"),
        &card.parts,
        None,
    )
    .expect("allowed");
    let sent = calls.lock().unwrap().clone();
    assert_eq!(sent[0].0, "install.apply");
    assert_eq!(sent[0].1["parts"], json!(["skill:claude:notchling-vault"]));
    assert_eq!(sent[0].1["consentId"], "c20");
}

#[test]
fn a_cost_card_sends_every_ticked_row() {
    let calls = Calls::default();
    let feed = feed_with(Ok(json!({})), &calls);
    let card = built(&cost_view(), &[]);
    feed.allow_family(
        "c20",
        card.family_apply.as_ref().expect("apply"),
        &card.parts,
        None,
    )
    .expect("allowed");
    let sent = calls.lock().unwrap()[0].1.clone();
    assert_eq!(sent["parts"], json!(["claude-settings", "profile"]));
}

#[test]
fn a_request_that_is_out_of_date_shows_as_stale() {
    let calls = Calls::default();
    let stale = ApiError::new(
        ErrorCode::Forbidden,
        "It no longer fits: the version moved.",
    )
    .with_reason("stale");
    let feed = feed_with(Err(stale), &calls);
    let card = built(&update_view(), &[]);
    let shown = feed
        .allow_family(
            "c20",
            card.family_apply.as_ref().expect("apply"),
            &card.parts,
            None,
        )
        .expect_err("stale");
    assert_eq!(shown, t::consent::install_stale_request());
    let other = feed_with(Err(ApiError::new(ErrorCode::Failed, "disk full")), &calls);
    assert_eq!(
        other
            .allow_family(
                "c20",
                card.family_apply.as_ref().expect("apply"),
                &card.parts,
                None
            )
            .expect_err("failed"),
        "disk full"
    );
}

#[test]
fn a_part_the_daemon_skipped_as_stale_is_named_in_the_note() {
    let calls = Calls::default();
    let reply = json!({"parts": [
        {"id": "hooks", "state": "stale", "error": null},
        {"id": "skill:claude:notchling-vault", "state": "written", "error": null}
    ]});
    let feed = feed_with(Ok(reply), &calls);
    let card = built(&install_view(), &[]);
    let note = feed
        .allow_family(
            "c20",
            card.family_apply.as_ref().expect("apply"),
            &card.parts,
            None,
        )
        .expect("allowed")
        .expect("a note");
    assert!(note.contains("/home/u/.claude/settings.json"));
    assert!(note.starts_with("Changed since this request"));
    assert!(!note.contains("notchling-vault"));
}

#[test]
fn a_machine_apply_carries_the_typed_password_once_and_the_card_never_holds_it() {
    let calls = Calls::default();
    let feed = feed_with(Ok(json!({})), &calls);
    let card = built(&machine_view(true), &[]);
    let typed = Admin::Password(notchling_engine::accounts::Secret::new("hunter2"));
    feed.allow_family(
        "c20",
        card.family_apply.as_ref().expect("apply"),
        &card.parts,
        Some(&typed),
    )
    .expect("allowed");
    let sent = calls.lock().unwrap().clone();
    assert_eq!(sent[0].0, "machines.apply");
    assert_eq!(sent[0].1["admin"], json!({"password": "hunter2"}));
    assert!(!format!("{card:?}").contains("hunter2"));
}

#[test]
fn a_detached_feed_says_so() {
    let feed = InboxFeed::none();
    let card = built(&update_view(), &[]);
    assert!(feed
        .allow_family(
            "c20",
            card.family_apply.as_ref().expect("apply"),
            &card.parts,
            None
        )
        .is_err());
}
