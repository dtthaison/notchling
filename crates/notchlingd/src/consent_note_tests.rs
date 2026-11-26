use super::tests::{at, opening, rig, Rig};
use super::*;
use crate::consent_testkit::{app_caller, call_op, item_id_for};
use crate::event_assert::{of, text};
use notchling_engine::consent::SETTING;
use std::sync::atomic::Ordering;

const NOTE: &str = "call it build-box-2 instead";

fn deny_with_note(r: &Rig, id: &str, note: &str) -> Value {
    call_op(
        &r.consent,
        &app_caller(),
        ops::CONSENT_ANSWER,
        json!({"consentId": id, "decision": "deny", "note": note}),
    )
}

fn retry(r: &Rig, pid: u32, target: &str, reason: &str, of: &str) -> Result<Opened, ApiError> {
    let mut asked = opening(SETTING, target, Some(reason));
    asked.retry_of = Some(of.to_string());
    r.consent.open(&at(pid), asked)
}

#[test]
fn a_noted_deny_is_stored_returned_to_the_asker_and_put_on_the_card() {
    let r = rig("note-deny");
    let row = r.open(1, SETTING, "k1");
    let item = item_id_for(&r.inbox, &row.id);
    let answered = deny_with_note(&r, &row.id, &format!("  {NOTE} \u{7}"));
    assert_eq!(answered["answered"], true);
    r.eventually_state(&row.id, "denied");
    let got = call_op(&r.consent, &at(1), ops::CONSENT_GET, json!({"id": row.id}));
    assert_eq!(got["state"], "denied");
    assert_eq!(got["note"], NOTE);
    let card = r.eventually_item(&row.id, "answered");
    assert_eq!(card["note"], NOTE, "{card}");
    assert_eq!(card["answer"], "deny", "{item}");
}

#[test]
fn a_note_goes_with_a_no_only_and_an_empty_one_is_refused() {
    let r = rig("note-allow");
    let row = r.open(1, SETTING, "k1");
    let allowed = call_op(
        &r.consent,
        &app_caller(),
        ops::CONSENT_ANSWER,
        json!({"consentId": row.id, "decision": "allow", "note": "x"}),
    );
    assert_eq!(allowed["code"], "bad_request");
    let empty = deny_with_note(&r, &row.id, "  \n ");
    assert_eq!(empty["code"], "bad_request");
    assert_eq!(r.state(&row.id), "waiting");
    let item = item_id_for(&r.inbox, &row.id);
    assert!(r
        .inbox
        .decide_noted(&item, ActionId::Allow, "mac", Some("x"))
        .is_err());
}

#[test]
fn the_inbox_answer_with_a_note_denies_with_it() {
    let r = rig("note-inbox");
    let row = r.open(1, SETTING, "k1");
    let item = item_id_for(&r.inbox, &row.id);
    let won = r
        .inbox
        .decide_noted(&item, ActionId::Deny, "mac", Some(NOTE))
        .unwrap();
    assert!(won.answered);
    r.eventually_state(&row.id, "denied");
    assert_eq!(
        r.store.get(&row.id).unwrap().unwrap().note.as_deref(),
        Some(NOTE)
    );
    assert_eq!(r.eventually_item(&row.id, "answered")["note"], NOTE);
}

#[test]
fn a_noted_deny_event_says_noted_and_no_field_holds_the_text() {
    let r = rig("note-events");
    let row = r.open(1, SETTING, "k1");
    deny_with_note(&r, &row.id, NOTE);
    r.eventually_state(&row.id, "denied");
    r.thens();
    for _ in 0..200 {
        if r.thens().contains(&"denied".to_string()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let events = of(&r.sink, "agent.consent");
    let denied = events
        .iter()
        .find(|e| text(e, "then").as_deref() == Some("denied"))
        .expect("a denied event");
    assert_eq!(text(denied, "noted").as_deref(), Some("true"));
    for event in r.sink.events() {
        let shown = format!("{event:?}");
        assert!(!shown.contains("build-box"), "{shown}");
    }
}

#[test]
fn a_changed_retry_of_a_noted_deny_opens_at_once_and_keeps_the_note() {
    let r = rig("retry-ok");
    let row = r.open(1, SETTING, "k1");
    deny_with_note(&r, &row.id, NOTE);
    r.eventually_state(&row.id, "denied");
    r.clock.fetch_add(60_000, Ordering::SeqCst);
    let again = retry(&r, 1, "k1", "the new name", &row.id).unwrap();
    let fresh = again.row();
    assert_eq!(fresh.state.as_str(), "waiting");
    assert_eq!(fresh.change["retryOf"], json!(row.id));
    assert_eq!(fresh.change["priorNote"], NOTE);
    assert_eq!(fresh.change["priorSummary"], "setting k1");
}

#[test]
fn a_retry_that_changes_nothing_or_follows_a_plain_deny_or_another_session_is_refused() {
    let r = rig("retry-no");
    let noted = r.open(1, SETTING, "k1");
    deny_with_note(&r, &noted.id, NOTE);
    r.eventually_state(&noted.id, "denied");
    let same = retry(&r, 1, "k1", "", &noted.id);
    let err = same.map(|_| ()).unwrap_err();
    assert_eq!(err.code.as_str(), "forbidden");
    let other = retry(&r, 2, "k1", "other", &noted.id)
        .map(|_| ())
        .unwrap_err();
    assert_eq!(other.code.as_str(), "forbidden");
    let plain = r.open(1, SETTING, "k2");
    r.consent.deny_by_app(&plain.id);
    r.eventually_state(&plain.id, "denied");
    let after_plain = retry(&r, 1, "k2", "other", &plain.id)
        .map(|_| ())
        .unwrap_err();
    assert_eq!(after_plain.code.as_str(), "forbidden");
    let missing = retry(&r, 1, "k3", "other", "cmissing")
        .map(|_| ())
        .unwrap_err();
    assert_eq!(missing.code.as_str(), "forbidden");
}

#[test]
fn a_denied_row_is_asked_again_once_even_after_the_retry_ended() {
    let r = rig("retry-once");
    let row = r.open(1, SETTING, "k1");
    deny_with_note(&r, &row.id, NOTE);
    r.eventually_state(&row.id, "denied");
    let first = retry(&r, 1, "k2", "one", &row.id).unwrap();
    let second = retry(&r, 1, "k3", "two", &row.id).map(|_| ()).unwrap_err();
    assert_eq!(second.code.as_str(), "forbidden");
    call_op(
        &r.consent,
        &at(1),
        ops::CONSENT_CANCEL,
        json!({"id": first.row().id}),
    );
    let third = retry(&r, 1, "k4", "three", &row.id)
        .map(|_| ())
        .unwrap_err();
    assert_eq!(third.code.as_str(), "forbidden");
}

#[test]
fn a_retry_of_a_retry_that_was_denied_with_a_note_is_a_chain_and_fine() {
    let r = rig("retry-chain");
    let row = r.open(1, SETTING, "k1");
    deny_with_note(&r, &row.id, NOTE);
    r.eventually_state(&row.id, "denied");
    let first = retry(&r, 1, "k2", "one", &row.id).unwrap().row().clone();
    deny_with_note(&r, &first.id, "closer");
    r.eventually_state(&first.id, "denied");
    retry(&r, 1, "k3", "two", &first.id).unwrap();
}

#[test]
fn an_inbox_answer_reads_the_note_as_strictly_as_the_app_answer() {
    let r = rig("inbox-strict");
    let row = r.open(1, SETTING, "k1");
    let item = item_id_for(&r.inbox, &row.id);
    let answer = |action: &str, note: Value| {
        use crate::op_group::OpGroup;
        r.inbox
            .call(
                &app_caller(),
                ops::INBOX_ANSWER,
                &json!({"id": item, "action": action, "note": note}),
            )
            .unwrap_or_else(|e| json!({"code": e.code.as_str()}))
    };
    for (action, note) in [
        ("allow", json!("x")),
        ("allow", json!("")),
        ("allow", json!(5)),
        ("deny", json!("  \n")),
        ("deny", json!("\u{7}")),
        ("deny", json!(5)),
    ] {
        let got = answer(action, note.clone());
        assert_eq!(got["code"], "bad_request", "{action} {note}");
    }
    assert_eq!(r.state(&row.id), "waiting");
    assert_eq!(answer("deny", json!("because"))["answered"], true);
}

#[test]
fn two_simultaneous_retries_of_one_denial_open_one_card_and_refuse_the_other() {
    let r = rig("retry-race");
    let row = r.open(1, SETTING, "k1");
    deny_with_note(&r, &row.id, NOTE);
    r.eventually_state(&row.id, "denied");
    let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
    let runs: Vec<_> = ["k2", "k3"]
        .into_iter()
        .map(|target| {
            let (consent, gate, old) = (r.consent.clone(), gate.clone(), row.id.clone());
            std::thread::spawn(move || {
                let mut asked = opening(SETTING, target, Some("again"));
                asked.retry_of = Some(old);
                gate.wait();
                consent.open(&at(1), asked).map(|_| ())
            })
        })
        .collect();
    let results: Vec<_> = runs.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|x| x.is_ok()).count(), 1);
    let err = results.into_iter().find_map(Result::err).unwrap();
    assert_eq!(err.code.as_str(), "forbidden");
}
