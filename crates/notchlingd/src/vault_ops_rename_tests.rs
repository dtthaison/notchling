use super::tests::{agent, app, ask, ended, err, seed, setup, setup_full, Setup};
use super::*;
use crate::event_assert::{of, results, text};
use std::sync::Mutex;

const OLD: &str = "revenuecat-webhook-secret";
const NEW: &str = "lingeva-revenuecat-webhook-secret";
const VALUE: &str = "fake-webhook-0042";

fn id_of(event: &Value) -> String {
    event["id"].as_str().unwrap().to_string()
}

fn save_as(s: &Setup, event: &Value, name: &str, extra: Value) -> Result<Value, ApiError> {
    let mut entry = event["entry"].clone();
    entry["name"] = json!(name);
    let mut args = json!({"entry": entry, "value": {"secret": VALUE}, "consentId": event["id"]});
    for (key, value) in extra.as_object().cloned().unwrap_or_default() {
        args[key] = value;
    }
    s.ops.call(&app(), ops::VAULT_SAVE, &args)
}

fn names(s: &Setup) -> Vec<String> {
    let list = s.ops.call(&app(), ops::VAULT_LIST, &json!({})).unwrap();
    let mut names: Vec<String> = list["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

fn session_of(current: &Arc<Mutex<&'static str>>) -> TrustedSession {
    let current = current.clone();
    Arc::new(move |_| Some((*current.lock().unwrap()).to_string()))
}

fn cancel_by_agent(s: &Setup, id: &str) {
    let done = s
        .consent
        .call(&agent(), ops::CONSENT_CANCEL, &json!({"id": id}))
        .unwrap();
    assert_eq!(done["cancelled"], true, "{done}");
}

fn asked_again(s: &Setup, name: &str) -> Value {
    let (_, event) = ask(s, json!({"name": name}));
    event
}

#[test]
fn a_request_is_saved_under_the_name_the_user_typed() {
    let s = setup("rename-save", true);
    let (reply, event) = ask(&s, json!({"name": OLD}));
    let id = id_of(&event);
    assert_eq!(reply["request"]["target"], OLD);
    let saved = save_as(&s, &event, NEW, json!({})).unwrap();
    assert_eq!(
        (saved["answered"].clone(), saved["entry"]["name"].clone()),
        (json!(true), json!(NEW))
    );
    assert_eq!(names(&s), [NEW]);
    let done = ended(&s, &id);
    assert_eq!(done["state"], "applied");
    assert_eq!(done["result"]["savedName"], NEW);
    assert_eq!(done["result"]["requestedName"], OLD);
    assert_eq!(done["result"]["entry"]["name"], NEW);
    assert!(!done.to_string().contains(VALUE));
    let used = s
        .ops
        .call(&agent(), ops::VAULT_FIND, &json!({"query": NEW}))
        .unwrap();
    assert!(used.to_string().contains(NEW), "{used}");
}

#[test]
fn a_save_under_the_asked_name_says_so_too() {
    let s = setup("rename-same", true);
    let (_, event) = ask(&s, json!({"name": OLD}));
    save_as(&s, &event, OLD, json!({})).unwrap();
    let done = ended(&s, &id_of(&event));
    assert_eq!(done["result"]["savedName"], OLD);
    assert_eq!(done["result"]["requestedName"], OLD);
}

#[test]
fn the_audit_names_the_asked_and_the_saved_name_and_never_the_value() {
    let s = setup("rename-audit", true);
    let (_, event) = ask(&s, json!({"name": OLD}));
    save_as(&s, &event, NEW, json!({})).unwrap();
    ended(&s, &id_of(&event));
    let asked = of(&s.sink, "vault.secret_requested");
    assert_eq!(asked.len(), 1);
    assert_eq!(text(&asked[0], "name").as_deref(), Some(OLD));
    assert_eq!(text(&asked[0], "saved_as").as_deref(), Some(NEW));
    assert_eq!(results(&s.sink, "vault.secret_requested"), ["saved"]);
    assert!(!format!("{:?}", s.sink.events()).contains(VALUE));
    assert!(!format!("{:?}", s.consent_sink.events()).contains(VALUE));
}

#[test]
fn the_audit_has_no_saved_as_when_the_name_did_not_change() {
    let s = setup("rename-audit-same", true);
    let (_, event) = ask(&s, json!({"name": OLD}));
    save_as(&s, &event, OLD, json!({})).unwrap();
    ended(&s, &id_of(&event));
    let asked = of(&s.sink, "vault.secret_requested");
    assert_eq!(text(&asked[0], "saved_as"), None);
}

#[test]
fn a_new_name_that_another_secret_holds_is_refused_and_the_request_stays_open() {
    let s = setup("rename-clash", true);
    seed(&s, false);
    let (_, event) = ask(&s, json!({"name": OLD}));
    let id = id_of(&event);
    let e = err(save_as(&s, &event, "gh", json!({})));
    assert_eq!(
        e.data.as_ref().unwrap()["reason"],
        json!("name_taken"),
        "{e:?}"
    );
    assert!(
        e.message.contains("already a secret named gh"),
        "{}",
        e.message
    );
    let row = s.consent.store().get(&id).unwrap().unwrap();
    assert!(row.state.open(), "{:?}", row.state);
    assert_eq!(names(&s), ["gh", "reg"]);
    let kept = s.ops.store.entry("gh").unwrap().unwrap();
    assert_eq!(
        s.ops.store.value(&kept).unwrap().unwrap().secret,
        "fake-token-value-0001"
    );
    save_as(&s, &event, NEW, json!({})).unwrap();
    assert_eq!(ended(&s, &id)["state"], "applied");
    assert_eq!(names(&s), ["gh", NEW, "reg"]);
}

#[test]
fn a_clash_ignores_the_case_of_the_name() {
    let s = setup("rename-clash-case", true);
    seed(&s, false);
    let (_, event) = ask(&s, json!({"name": OLD}));
    let e = err(save_as(&s, &event, "GH", json!({})));
    assert_eq!(e.data.as_ref().unwrap()["reason"], json!("name_taken"));
    assert!(s
        .consent
        .store()
        .get(&id_of(&event))
        .unwrap()
        .unwrap()
        .state
        .open());
}

#[test]
fn a_secret_that_appeared_under_the_asked_name_is_not_overwritten_by_a_save_under_it() {
    let s = setup("rename-appeared", true);
    let (_, event) = ask(&s, json!({"name": "gh"}));
    seed(&s, false);
    let e = err(save_as(&s, &event, "gh", json!({})));
    assert_eq!(e.code, ErrorCode::Forbidden);
    let kept = s.ops.store.entry("gh").unwrap().unwrap();
    assert_eq!(
        s.ops.store.value(&kept).unwrap().unwrap().secret,
        "fake-token-value-0001"
    );
}

#[test]
fn an_existing_secret_can_be_renamed_through_its_request() {
    let s = setup("rename-existing", true);
    seed(&s, false);
    let (_, event) = ask(&s, json!({"name": "gh", "note": "renewed"}));
    assert_eq!(event["existing"], true);
    save_as(&s, &event, "github", json!({"replacing": "gh"})).unwrap();
    let done = ended(&s, &id_of(&event));
    assert_eq!(done["state"], "applied");
    assert_eq!(done["result"]["savedName"], "github");
    assert_eq!(done["result"]["requestedName"], "gh");
    assert_eq!(names(&s), ["github", "reg"]);
}

#[test]
fn an_existing_secret_cannot_be_renamed_onto_another_one() {
    let s = setup("rename-existing-clash", true);
    seed(&s, false);
    let (_, event) = ask(&s, json!({"name": "gh"}));
    let e = err(save_as(&s, &event, "reg", json!({"replacing": "gh"})));
    assert_eq!(e.data.as_ref().unwrap()["reason"], json!("name_taken"));
    assert!(s
        .consent
        .store()
        .get(&id_of(&event))
        .unwrap()
        .unwrap()
        .state
        .open());
    assert_eq!(names(&s), ["gh", "reg"]);
}

#[test]
fn a_renamed_save_still_checks_that_the_secret_shown_has_not_changed() {
    let s = setup("rename-drift", true);
    seed(&s, false);
    let (_, event) = ask(&s, json!({"name": "gh", "note": "renewed"}));
    let shown = s.ops.store.entry("gh").unwrap().unwrap();
    let value = notchling_engine::vault::VaultValue {
        secret: "fake-gh-0002".into(),
        ..Default::default()
    };
    s.ops
        .store
        .save(&shown, Some(value), Some(&shown), &shown.updated)
        .unwrap();
    let e = err(save_as(&s, &event, "github", json!({"replacing": "gh"})));
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert_eq!(names(&s), ["gh", "reg"]);
    assert_eq!(ended(&s, &id_of(&event))["state"], "withdrawn");
}

#[test]
fn the_kind_of_the_request_is_still_checked() {
    let s = setup("rename-kind", true);
    let (_, event) = ask(&s, json!({"name": OLD}));
    let other = s
        .consent
        .store()
        .insert(
            &notchling_engine::consent::New {
                kind: "setting".into(),
                target: "x".into(),
                change: json!({}),
                facts: json!([]),
                request: json!({}),
                parts: None,
                reason: None,
                session: Some("sess-x".into()),
                tool: "claude".into(),
                verified: true,
                root: None,
                folder: None,
                organization: None,
                pid: 0,
                pid_start: None,
                idem_key: None,
                answer_on: notchling_engine::consent::AnswerOn::Mac,
                applied_by: notchling_engine::consent::AppliedBy::App,
                expires_in_ms: 60_000,
            },
            s.consent.now(),
        )
        .unwrap();
    let mut entry = event["entry"].clone();
    entry["name"] = json!(NEW);
    let wrong = s.ops.call(
        &app(),
        ops::VAULT_SAVE,
        &json!({"entry": entry, "value": {"secret": VALUE}, "consentId": other.row().id}),
    );
    let e = err(wrong);
    assert_eq!(e.message, "That request is for something else.");
    assert_eq!(names(&s), Vec::<String>::new());
}

#[test]
fn a_request_that_ran_out_of_time_says_expired_even_before_the_clock_marks_it() {
    let s = setup("rename-expired-window", true);
    let (_, event) = ask(&s, json!({"name": OLD, "consent": {"expiresInMs": 60_000}}));
    s.drift(61_000);
    let e = err(save_as(&s, &event, NEW, json!({})));
    let data = e.data.clone().unwrap();
    assert_eq!(data["reason"], "needs_consent");
    assert_eq!(data["state"], "expired", "{e:?}");
    assert_eq!(e.message, "That request ran out of time.");
    assert_eq!(names(&s), Vec::<String>::new());
}

#[test]
fn a_request_the_clock_already_expired_says_expired() {
    let s = setup("rename-expired-ticked", true);
    let (_, event) = ask(&s, json!({"name": OLD, "consent": {"expiresInMs": 60_000}}));
    s.advance(61_000);
    let e = err(save_as(&s, &event, NEW, json!({})));
    assert_eq!(e.data.clone().unwrap()["state"], "expired", "{e:?}");
    assert_eq!(ended(&s, &id_of(&event))["state"], "expired");
}

#[test]
fn a_request_that_was_cancelled_says_what_it_is() {
    let s = setup("rename-cancelled", true);
    let (_, event) = ask(&s, json!({"name": OLD}));
    s.ops
        .call(
            &app(),
            ops::VAULT_REQUEST_CANCEL,
            &json!({"id": event["id"]}),
        )
        .unwrap();
    let e = err(save_as(&s, &event, NEW, json!({})));
    let data = e.data.clone().unwrap();
    assert_eq!(
        (data["reason"].clone(), data["state"].clone()),
        (json!("needs_consent"), json!("denied"))
    );
}

#[test]
fn a_re_ask_from_the_same_session_is_answered_by_one_save_of_the_older_form() {
    let who = Arc::new(Mutex::new("sess-a"));
    let s = setup_full("reask-both", true, None, None, Some(session_of(&who)));
    let first = asked_again(&s, OLD);
    let second = asked_again(&s, OLD);
    assert_ne!(first["id"], second["id"]);
    let saved = save_as(&s, &first, NEW, json!({})).unwrap();
    assert_eq!(saved["answered"], true);
    for event in [&first, &second] {
        let done = ended(&s, &id_of(event));
        assert_eq!(done["state"], "applied", "{done}");
        assert_eq!(done["result"]["savedName"], NEW);
    }
    assert_eq!(names(&s), [NEW]);
    assert!(s.ops.pending()["requests"].as_array().unwrap().is_empty());
}

#[test]
fn a_form_whose_request_was_cancelled_saves_into_the_newer_request() {
    let who = Arc::new(Mutex::new("sess-a"));
    let s = setup_full("reask-moved", true, None, None, Some(session_of(&who)));
    let first = asked_again(&s, OLD);
    cancel_by_agent(&s, &id_of(&first));
    let second = asked_again(&s, OLD);
    let saved = save_as(&s, &first, NEW, json!({})).unwrap();
    assert_eq!(saved["answered"], true);
    assert_eq!(ended(&s, &id_of(&second))["state"], "applied");
    assert_eq!(ended(&s, &id_of(&second))["result"]["savedName"], NEW);
    assert_eq!(ended(&s, &id_of(&first))["state"], "cancelled");
    assert_eq!(names(&s), [NEW]);
}

#[test]
fn a_form_whose_request_expired_saves_into_the_newer_request() {
    let who = Arc::new(Mutex::new("sess-a"));
    let s = setup_full("reask-expired", true, None, None, Some(session_of(&who)));
    let (_, first) = ask(&s, json!({"name": OLD, "consent": {"expiresInMs": 60_000}}));
    s.advance(61_000);
    assert_eq!(ended(&s, &id_of(&first))["state"], "expired");
    let second = asked_again(&s, OLD);
    save_as(&s, &first, NEW, json!({})).unwrap();
    assert_eq!(ended(&s, &id_of(&second))["state"], "applied");
}

#[test]
fn a_request_from_another_session_or_for_another_name_is_left_alone() {
    let who = Arc::new(Mutex::new("sess-a"));
    let s = setup_full("reask-others", true, None, None, Some(session_of(&who)));
    let mine = asked_again(&s, OLD);
    let other_name = asked_again(&s, "other-secret");
    *who.lock().unwrap() = "sess-b";
    let other_session = asked_again(&s, OLD);
    save_as(&s, &mine, NEW, json!({})).unwrap();
    assert_eq!(ended(&s, &id_of(&mine))["state"], "applied");
    for event in [&other_name, &other_session] {
        let row = s.consent.store().get(&id_of(event)).unwrap().unwrap();
        assert!(row.state.open(), "{:?}", row.state);
    }
}

#[test]
fn a_form_whose_request_ended_with_no_newer_one_keeps_its_own_refusal() {
    let who = Arc::new(Mutex::new("sess-a"));
    let s = setup_full("reask-none", true, None, None, Some(session_of(&who)));
    let first = asked_again(&s, OLD);
    cancel_by_agent(&s, &id_of(&first));
    let e = err(save_as(&s, &first, NEW, json!({})));
    let data = e.data.clone().unwrap();
    assert_eq!(
        (data["reason"].clone(), data["state"].clone()),
        (json!("needs_consent"), json!("cancelled"))
    );
    assert_eq!(names(&s), Vec::<String>::new());
}

fn sessions_that_come_and_go() -> (Arc<Mutex<Option<&'static str>>>, TrustedSession) {
    let who: Arc<Mutex<Option<&'static str>>> = Arc::new(Mutex::new(Some("sess-a")));
    let seen = who.clone();
    let sessions: TrustedSession = Arc::new(move |_| (*seen.lock().unwrap()).map(str::to_string));
    (who, sessions)
}

#[test]
fn a_verified_form_does_not_answer_the_re_ask_of_a_caller_without_a_verified_session() {
    let (who, sessions) = sessions_that_come_and_go();
    let s = setup_full("reask-unverified-twin", true, None, None, Some(sessions));
    let verified = asked_again(&s, OLD);
    *who.lock().unwrap() = None;
    let anonymous = asked_again(&s, OLD);
    save_as(&s, &verified, NEW, json!({})).unwrap();
    assert_eq!(ended(&s, &id_of(&verified))["state"], "applied");
    let row = s.consent.store().get(&id_of(&anonymous)).unwrap().unwrap();
    assert!(row.state.open(), "{:?}", row.state);
}

#[test]
fn a_form_of_a_caller_without_a_verified_session_answers_no_other_request() {
    let (who, sessions) = sessions_that_come_and_go();
    let s = setup_full("reask-unverified-primary", true, None, None, Some(sessions));
    *who.lock().unwrap() = None;
    let anonymous = asked_again(&s, OLD);
    *who.lock().unwrap() = Some("sess-a");
    let verified = asked_again(&s, OLD);
    save_as(&s, &anonymous, NEW, json!({})).unwrap();
    assert_eq!(ended(&s, &id_of(&anonymous))["state"], "applied");
    let row = s.consent.store().get(&id_of(&verified)).unwrap().unwrap();
    assert!(row.state.open(), "{:?}", row.state);
}

fn change_secret(s: &Setup, name: &str, secret: &str) {
    let shown = s.ops.store.entry(name).unwrap().unwrap();
    let value = notchling_engine::vault::VaultValue {
        secret: secret.into(),
        ..Default::default()
    };
    s.ops
        .store
        .save(&shown, Some(value), Some(&shown), &shown.updated)
        .unwrap();
}

fn step_of(s: &Setup, event: &Value) -> notchling_engine::consent::State {
    s.consent.store().get(&id_of(event)).unwrap().unwrap().state
}

#[test]
fn a_form_opened_before_a_change_cannot_overwrite_it_through_a_newer_request() {
    let who = Arc::new(Mutex::new("sess-a"));
    let s = setup_full("stale-form", true, None, None, Some(session_of(&who)));
    seed(&s, false);
    let (_, first) = ask(
        &s,
        json!({"name": "gh", "consent": {"expiresInMs": 60_000}}),
    );
    assert_eq!(first["existing"], true);
    change_secret(&s, "gh", "fake-gh-0002");
    s.advance(61_000);
    let second = asked_again(&s, "gh");
    let e = err(save_as(&s, &first, "gh", json!({"replacing": "gh"})));
    assert_eq!(e.code, ErrorCode::Forbidden);
    let kept = s.ops.store.entry("gh").unwrap().unwrap();
    assert_eq!(
        s.ops.store.value(&kept).unwrap().unwrap().secret,
        "fake-gh-0002"
    );
    assert!(step_of(&s, &second).open());
}

#[test]
fn a_form_opened_at_the_current_state_still_saves_into_the_newer_request_of_an_existing_secret() {
    let who = Arc::new(Mutex::new("sess-a"));
    let s = setup_full("fresh-form", true, None, None, Some(session_of(&who)));
    seed(&s, false);
    let (_, first) = ask(
        &s,
        json!({"name": "gh", "consent": {"expiresInMs": 60_000}}),
    );
    s.advance(61_000);
    let second = asked_again(&s, "gh");
    save_as(&s, &first, "gh", json!({"replacing": "gh"})).unwrap();
    assert_eq!(ended(&s, &id_of(&second))["state"], "applied");
}

#[test]
fn a_request_recorded_after_a_change_is_not_joined_to_the_older_form_and_stays_open() {
    let who = Arc::new(Mutex::new("sess-a"));
    let s = setup_full("stale-twin", true, None, None, Some(session_of(&who)));
    seed(&s, false);
    let first = asked_again(&s, "gh");
    let same = asked_again(&s, "gh");
    change_secret(&s, "gh", "fake-gh-0002");
    let later = asked_again(&s, "gh");
    let mut answering = s
        .ops
        .begin_answer(&s.consent, &app(), &id_of(&first))
        .unwrap();
    answering.join_twins(&app());
    use notchling_engine::consent::State as Step;
    assert_eq!(step_of(&s, &first), Step::Applying);
    assert_eq!(step_of(&s, &same), Step::Applying);
    assert_eq!(step_of(&s, &later), Step::Waiting);
    answering.withdraw();
    assert_eq!(step_of(&s, &first), Step::Withdrawn);
    assert_eq!(step_of(&s, &same), Step::Withdrawn);
    assert_eq!(step_of(&s, &later), Step::Waiting);
}

#[test]
fn a_taken_name_leaves_the_request_and_its_twins_waiting_for_another_name() {
    let who = Arc::new(Mutex::new("sess-a"));
    let s = setup_full("clash-twins", true, None, None, Some(session_of(&who)));
    seed(&s, false);
    let first = asked_again(&s, OLD);
    let second = asked_again(&s, OLD);
    let e = err(save_as(&s, &first, "gh", json!({})));
    assert_eq!(e.data.as_ref().unwrap()["reason"], json!("name_taken"));
    use notchling_engine::consent::State as Step;
    for event in [&first, &second] {
        assert_eq!(step_of(&s, event), Step::Waiting);
    }
    save_as(&s, &first, NEW, json!({})).unwrap();
    for event in [&first, &second] {
        assert_eq!(ended(&s, &id_of(event))["state"], "applied");
    }
    assert_eq!(names(&s), ["gh", NEW, "reg"]);
}
