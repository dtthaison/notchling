use super::tests::{args_for, job, member, reason, rig, rig_as, Rig};
use super::*;
use crate::consent_testkit::verified;
use crate::job_book::KEEP_ROWS;
use notchling_engine::organizations::PERSONAL_ID;
use std::sync::atomic::Ordering;
use std::time::Duration;

fn members() -> Vec<Member> {
    vec![
        member("0123456789abcdef", "build", "Personal"),
        member("fedcba9876543210", "abroad", "Work"),
    ]
}

fn open_cards(r: &Rig) -> usize {
    r.consent.store().open_rows().unwrap().len()
}

fn decide_quietly(r: &Rig, id: &str) -> Consent {
    let _ = r.consent.decide(id, true, None, "mac", None);
    r.consent.wait_for(id, Duration::from_secs(5)).unwrap()
}

fn delegate_record(started: i64) -> Record {
    Record {
        session: "parent".to_string(),
        organization: "Personal".to_string(),
        kind: "delegate".to_string(),
        tool: "agy".to_string(),
        folder: "/work/a".to_string(),
        started_ms: started,
        ended_ms: None,
        exit: None,
    }
}

#[test]
fn an_unverified_caller_cannot_dispatch_or_act_on_any_job() {
    let r = rig_as("unverified", AgentIdentity::shell(), members());
    let e = r
        .call(ops::CLUSTER_DISPATCH, args_for("shell", "u"))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert_eq!(reason(&e), "no_session");
    for op in [ops::JOBS_TAIL, ops::JOBS_RETURN, ops::JOBS_STOP] {
        let e = r.call(op, json!({"id": "j-other"})).unwrap_err();
        assert_eq!(reason(&e), "no_session", "{op}");
    }
    let e = r.call(ops::JOBS_GET, json!({"id": "j-other"})).unwrap_err();
    assert_eq!(reason(&e), "no_job");
    assert_eq!(
        r.call(ops::JOBS_MINE, json!({})).unwrap()["jobs"],
        json!([])
    );
    assert_eq!(open_cards(&r), 0);
    assert!(r.ran().is_empty());
}

#[test]
fn a_verified_caller_with_no_organization_is_personal_and_never_a_wildcard() {
    let mut me = verified("parent");
    me.organization = None;
    let r = rig_as(
        "no-org",
        me,
        vec![
            member("0123456789abcdef", "build", PERSONAL_ID),
            member("fedcba9876543210", "abroad", "Work"),
        ],
    );
    *r.fleet.jobs.lock().unwrap() = vec![job("j-p", PERSONAL_ID), job("j-w", "Work")];
    let mut abroad = args_for("shell", "abroad");
    abroad["machine"] = json!("abroad");
    abroad["node"] = json!("fedcba9876543210");
    let e = r.call(ops::CLUSTER_DISPATCH, abroad).unwrap_err();
    assert_eq!(reason(&e), "other_organization");
    let own = r
        .call(ops::CLUSTER_DISPATCH, args_for("shell", "home"))
        .unwrap();
    assert_eq!(own["state"], "waiting");
    let e = r.call(ops::JOBS_GET, json!({"id": "j-w"})).unwrap_err();
    assert_eq!(reason(&e), "no_job");
    let got = r.call(ops::JOBS_GET, json!({"id": "j-p"})).unwrap();
    assert_eq!(got, json!({"id": "j-p", "own": false, "state": "running"}));
    let listed = r.call(ops::JOBS_MINE, json!({})).unwrap();
    let ids: Vec<&str> = listed["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|j| j["id"].as_str())
        .collect();
    assert_eq!(ids, ["j-p"]);
    for op in [ops::JOBS_TAIL, ops::JOBS_RETURN, ops::JOBS_STOP] {
        let e = r.call(op, json!({"id": "j-w"})).unwrap_err();
        assert_eq!(reason(&e), "other_organization", "{op}");
    }
}

#[test]
fn another_session_reusing_a_key_does_not_take_the_job() {
    let r = rig("key-owner");
    let first = r
        .call(ops::CLUSTER_DISPATCH, args_for("shell", "shared"))
        .unwrap();
    let card = r.card_of(&first);
    r.allow(&card, "mac");
    let started = job_id("shared");
    assert!(r.asks.book().owned_by("parent", &started).is_some());
    r.fleet.jobs.lock().unwrap().push(job(&started, "Personal"));
    r.as_session("intruder", Some("Personal"));
    let mut benign = args_for("shell", "shared");
    benign["command"] = json!("echo ok");
    let e = r.call(ops::CLUSTER_DISPATCH, benign).unwrap_err();
    assert_eq!(reason(&e), "key_in_use");
    assert_eq!(open_cards(&r), 0);
    assert!(r.asks.book().owned_by("parent", &started).is_some());
    assert!(r.asks.book().owned_by("intruder", &started).is_none());
    assert_eq!(r.ran(), ["dispatch"]);
}

#[test]
fn a_card_opened_before_the_owner_was_allowed_cannot_take_the_job_either() {
    let r = rig("key-race");
    r.as_session("late", Some("Personal"));
    let late = r
        .call(ops::CLUSTER_DISPATCH, args_for("shell", "raced"))
        .unwrap();
    let late_card = r.card_of(&late);
    r.as_session("parent", Some("Personal"));
    let first = r
        .call(ops::CLUSTER_DISPATCH, args_for("shell", "raced"))
        .unwrap();
    let first_card = r.card_of(&first);
    r.allow(&first_card, "mac");
    let row = decide_quietly(&r, &late_card);
    assert_ne!(row.state.as_str(), "applied");
    let started = job_id("raced");
    assert!(r.asks.book().owned_by("parent", &started).is_some());
    assert!(r.asks.book().owned_by("late", &started).is_none());
    assert_eq!(r.ran(), ["dispatch"]);
}

#[test]
fn reading_jobs_needs_the_cluster_switch_too() {
    let r = rig("reads-off");
    r.on.store(false, Ordering::SeqCst);
    let e = r.call(ops::JOBS_MINE, json!({})).unwrap_err();
    assert_eq!(reason(&e), "switch_off");
    let e = r.call(ops::JOBS_GET, json!({"id": "j-own"})).unwrap_err();
    assert_eq!(reason(&e), "switch_off");
}

#[test]
fn delegate_rows_never_push_a_dispatch_row_out_of_the_book() {
    let book = JobBook::in_memory();
    let mut dispatched = delegate_record(1);
    dispatched.kind = "dispatch".to_string();
    book.record("j-running", dispatched);
    for n in 0..(KEEP_ROWS + 20) {
        book.record(&format!("d-{n}"), delegate_record(10 + n as i64));
    }
    assert!(book.owned_by("parent", "j-running").is_some());
    let delegates = book
        .all()
        .iter()
        .filter(|(_, r)| r.kind == "delegate")
        .count();
    assert!(delegates <= KEEP_ROWS, "{delegates}");
    assert!(delegates >= KEEP_ROWS - 1, "{delegates}");
}

#[test]
fn a_claim_never_overwrites_another_sessions_row() {
    let book = JobBook::in_memory();
    let mut mine = delegate_record(1);
    mine.kind = "dispatch".to_string();
    assert!(book.claim("j-1", mine.clone()));
    assert!(book.claim("j-1", mine.clone()));
    let mut theirs = mine;
    theirs.session = "other".to_string();
    assert!(!book.claim("j-1", theirs));
    assert_eq!(book.get("j-1").unwrap().session, "parent");
}

#[test]
fn a_card_checks_the_job_again_when_the_user_allows() {
    let r = rig("recheck");
    let stop = r.call(ops::JOBS_STOP, json!({"id": "j-other"})).unwrap();
    let card = r.card_of(&stop);
    r.fleet
        .jobs
        .lock()
        .unwrap()
        .iter_mut()
        .find(|j| j.id == "j-other")
        .unwrap()
        .organization = "Work".to_string();
    assert_ne!(decide_quietly(&r, &card).state.as_str(), "applied");
    r.fleet
        .jobs
        .lock()
        .unwrap()
        .push(job("j-third", "Personal"));
    let stop = r.call(ops::JOBS_STOP, json!({"id": "j-third"})).unwrap();
    let card = r.card_of(&stop);
    r.fleet
        .jobs
        .lock()
        .unwrap()
        .iter_mut()
        .find(|j| j.id == "j-third")
        .unwrap()
        .state = "done".to_string();
    assert_ne!(decide_quietly(&r, &card).state.as_str(), "applied");
    assert!(r.ran().is_empty(), "{:?}", r.ran());
}

#[test]
fn a_stop_that_cannot_work_opens_no_card() {
    let r = rig("stop-fits");
    let mut ended = job("j-ended", "Personal");
    ended.state = "done".to_string();
    let mut session = job("j-session", "Personal");
    session.tool = "claude".to_string();
    r.fleet.jobs.lock().unwrap().extend([ended, session]);
    let e = r
        .call(ops::JOBS_STOP, json!({"id": "j-ended"}))
        .unwrap_err();
    assert_eq!(reason(&e), "job_ended");
    let e = r
        .call(ops::JOBS_STOP, json!({"id": "j-session"}))
        .unwrap_err();
    assert_eq!(reason(&e), "not_yet");
    assert_eq!(open_cards(&r), 0);
    let tail = r.call(ops::JOBS_TAIL, json!({"id": "j-ended"})).unwrap();
    assert_eq!(tail["state"], "waiting");
}

#[test]
fn a_machine_going_online_or_offline_does_not_break_the_same_key() {
    let r = rig("online");
    let first = r
        .call(ops::CLUSTER_DISPATCH, args_for("shell", "flip"))
        .unwrap();
    let card = r.card_of(&first);
    r.fleet.members.lock().unwrap()[0].online = false;
    let again = r
        .call(ops::CLUSTER_DISPATCH, args_for("shell", "flip"))
        .unwrap();
    assert_eq!(r.card_of(&again), card);
    assert_eq!(open_cards(&r), 1);
}
