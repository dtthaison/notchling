use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_ops::Opening;
use crate::consent_testkit::{agent_caller, app_caller, consent};
use crate::inbox_ops::InboxOps;
use crate::limits_ops_tests::TempDir;
use notchling_engine::consent::{COST_PROFILE, INSTALL, MACHINE, SETTING};
use notchling_events::MemorySink;
use notchling_local_api::{ops, ErrorCode};
use op_group_shim::call;
use std::sync::Arc;

mod op_group_shim {
    use crate::consent_ops::ConsentOps;
    use crate::op_group::OpGroup;
    use notchling_local_api::{ApiError, Caller};
    use serde_json::Value;

    pub fn call(c: &ConsentOps, caller: &Caller, op: &str, args: Value) -> Result<Value, ApiError> {
        c.call(caller, op, &args)
    }
}

struct Rig {
    _tmp: TempDir,
    consent: Arc<ConsentOps>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.consent.close();
    }
}

fn rig(tag: &str) -> Rig {
    let tmp = TempDir::new(&format!("consent-parts-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let consent = consent(room, inbox, Arc::new(MemorySink::new()));
    Rig { _tmp: tmp, consent }
}

fn offered() -> Value {
    json!([
        {"id": "hooks", "path": "settings.json"},
        {"id": "skill:claude:notchling-vault", "path": "skills/notchling-vault"},
        {"id": "sidecar", "path": "config.json"},
    ])
}

fn opening(kind: &'static str, parts: Option<Value>) -> Opening {
    Opening {
        parts,
        kind,
        target: "t".to_string(),
        change: json!({"action": "x", "apply": {"op": "x", "args": {}}}),
        facts: json!([]),
        request: json!({"op": "x"}),
        reason: None,
        expires_in_ms: None,
        key: None,
        retry_of: None,
    }
}

impl Rig {
    fn open(&self, kind: &'static str, parts: Option<Value>) -> Result<Consent, ApiError> {
        self.consent
            .open(&agent_caller(), opening(kind, parts))
            .map(|opened| opened.row().clone())
    }

    fn answer(&self, id: &str, decision: &str, parts: Option<Value>) -> Result<Value, ApiError> {
        let mut args = json!({"consentId": id, "decision": decision});
        if let Some(parts) = parts {
            args["parts"] = parts;
        }
        call(&self.consent, &app_caller(), ops::CONSENT_ANSWER, args)
    }

    fn take(&self, id: &str, kind: &str, parts: Option<Value>) -> Result<Consent, ApiError> {
        self.consent
            .take_for_app_keeping(&app_caller(), id, kind, None, parts.as_ref())
    }

    fn row(&self, id: &str) -> Consent {
        self.consent.get_row(id).unwrap()
    }
}

fn reason(e: &ApiError) -> Option<&str> {
    e.data.as_ref()?.get("reason")?.as_str()
}

#[test]
fn an_answer_naming_a_part_the_card_does_not_have_is_refused() {
    let r = rig("outside");
    let id = r.open(INSTALL, Some(offered())).unwrap().id;
    let e = r
        .answer(&id, "allow", Some(json!(["hooks", "skill:codex:other"])))
        .unwrap_err();
    assert_eq!(
        (e.code, reason(&e)),
        (ErrorCode::BadRequest, Some("bad_value"))
    );
    assert_eq!(r.row(&id).state.as_str(), "waiting");
    assert!(r.row(&id).parts_allowed.is_none());
}

#[test]
fn a_parts_kind_answered_with_no_parts_or_an_empty_set_is_refused() {
    let r = rig("empty");
    for kind in [INSTALL, COST_PROFILE] {
        let id = r.open(kind, Some(offered())).unwrap().id;
        for parts in [
            None,
            Some(json!([])),
            Some(json!("hooks")),
            Some(json!([1])),
        ] {
            let e = r.answer(&id, "allow", parts.clone()).unwrap_err();
            assert_eq!(reason(&e), Some("bad_value"), "{kind} {parts:?}");
        }
        assert_eq!(r.row(&id).state.as_str(), "waiting");
    }
}

#[test]
fn parts_on_a_kind_without_parts_are_refused() {
    let r = rig("plain");
    for kind in [MACHINE, SETTING] {
        let id = r.open(kind, None).unwrap().id;
        let e = r.answer(&id, "allow", Some(json!(["hooks"]))).unwrap_err();
        assert_eq!(reason(&e), Some("bad_value"), "{kind}");
        assert_eq!(r.row(&id).state.as_str(), "waiting");
    }
}

#[test]
fn a_deny_carries_no_parts() {
    let r = rig("deny");
    let id = r.open(INSTALL, Some(offered())).unwrap().id;
    let e = r.answer(&id, "deny", Some(json!(["hooks"]))).unwrap_err();
    assert_eq!(reason(&e), Some("bad_value"));
    let said = r.answer(&id, "deny", None).unwrap();
    assert_eq!(said["state"], "denied");
}

#[test]
fn a_subset_is_kept_in_the_cards_order_and_stored_as_the_allowed_parts() {
    let r = rig("subset");
    let id = r.open(INSTALL, Some(offered())).unwrap().id;
    let taken = r
        .take(&id, INSTALL, Some(json!(["sidecar", "hooks", "hooks"])))
        .unwrap();
    assert_eq!(taken.state.as_str(), "applying");
    assert_eq!(taken.parts_allowed, Some(json!(["hooks", "sidecar"])));
    assert_eq!(r.row(&id).parts_allowed, Some(json!(["hooks", "sidecar"])));
}

#[test]
fn an_answer_to_a_plain_kind_without_parts_still_works() {
    let r = rig("still");
    let id = r.open(MACHINE, None).unwrap().id;
    let taken = r.take(&id, MACHINE, None).unwrap();
    assert_eq!(taken.state.as_str(), "applying");
    assert!(taken.parts_allowed.is_none());
}

#[test]
fn kept_parts_refuses_an_id_outside_the_allowed_parts() {
    let r = rig("kept");
    let id = r.open(COST_PROFILE, Some(offered())).unwrap().id;
    r.take(&id, COST_PROFILE, Some(json!(["hooks", "sidecar"])))
        .unwrap();
    let row = r.row(&id);
    let named = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        ConsentOps::kept_parts(&row, &named(&["sidecar"])).unwrap(),
        vec!["sidecar"]
    );
    assert_eq!(
        ConsentOps::kept_parts(&row, &named(&["sidecar", "hooks"])).unwrap(),
        vec!["hooks", "sidecar"]
    );
    for bad in [
        named(&["skill:claude:notchling-vault"]),
        named(&["hooks", "skill:claude:notchling-vault"]),
        named(&[]),
    ] {
        let e = ConsentOps::kept_parts(&row, &bad).unwrap_err();
        assert_eq!(
            (e.code, reason(&e)),
            (ErrorCode::Forbidden, Some("needs_consent")),
            "{bad:?}"
        );
    }
}

#[test]
fn kept_parts_refuses_a_row_that_was_never_answered_or_has_no_parts() {
    let r = rig("unanswered");
    let with = r.open(INSTALL, Some(offered())).unwrap();
    let without = r.open(MACHINE, None).unwrap();
    for row in [with, without] {
        let e = ConsentOps::kept_parts(&row, &["hooks".to_string()]).unwrap_err();
        assert_eq!(reason(&e), Some("needs_consent"));
    }
}

#[test]
fn kept_parts_refuses_a_row_that_was_put_back_after_a_failed_apply() {
    let r = rig("put-back");
    let id = r.open(INSTALL, Some(offered())).unwrap().id;
    let taken = r.take(&id, INSTALL, Some(json!(["hooks"]))).unwrap();
    assert!(ConsentOps::kept_parts(&taken, &["hooks".to_string()]).is_ok());
    let back = r
        .consent
        .store()
        .reopen(&id, notchling_engine::consent::State::Opened)
        .unwrap()
        .unwrap();
    assert!(back.parts_allowed.is_none());
    assert!(r.row(&id).parts_allowed.is_none());
    let e = ConsentOps::kept_parts(&back, &["hooks".to_string()]).unwrap_err();
    assert_eq!(reason(&e), Some("needs_consent"));
}

#[test]
fn kept_parts_refuses_a_row_that_is_not_in_use_whatever_it_holds() {
    use notchling_engine::consent::State;
    let r = rig("state");
    let id = r.open(INSTALL, Some(offered())).unwrap().id;
    let taken = r.take(&id, INSTALL, Some(json!(["hooks"]))).unwrap();
    for state in State::ALL {
        let mut row = taken.clone();
        row.state = state;
        let allowed = matches!(state, State::Applying | State::Applied);
        let out = ConsentOps::kept_parts(&row, &["hooks".to_string()]);
        assert_eq!(out.is_ok(), allowed, "{state:?}");
        if let Err(e) = out {
            assert_eq!(reason(&e), Some("needs_consent"), "{state:?}");
        }
    }
}

#[test]
fn opening_a_request_checks_its_parts_against_its_kind() {
    let r = rig("opening");
    for (kind, parts) in [
        (INSTALL, None),
        (INSTALL, Some(json!([]))),
        (INSTALL, Some(json!([{"path": "a"}]))),
        (INSTALL, Some(json!([{"id": "a"}, {"id": "a"}]))),
        (INSTALL, Some(json!([{"id": ""}]))),
        (MACHINE, Some(json!([{"id": "a"}]))),
    ] {
        let e = r.open(kind, parts.clone()).unwrap_err();
        assert_eq!(e.code, ErrorCode::Internal, "{kind} {parts:?}");
    }
    assert!(r.open(INSTALL, Some(json!([{"id": "a"}]))).is_ok());
}

#[test]
fn the_row_keeps_its_parts_for_the_card() {
    let r = rig("kept-row");
    let row = r.open(INSTALL, Some(offered())).unwrap();
    assert_eq!(row.parts, Some(offered()));
    assert_eq!(ids_of(&offered()).unwrap().len(), 3);
}
