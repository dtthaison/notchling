use crate::cluster_move_tests::{logged, one, Spec, S2};
use crate::cluster_promote_kit::{rig, rig_with, Rig, CUT_PUSH};
use crate::inbox_ops_tests::item;
use crate::snap_restore_testkit::S1;
use notchling_engine::cluster::claims::Claim;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::move_home::NO_PROOF;
use notchling_engine::cluster::state::Command;
use notchling_engine::cluster::wire::WireError;
use notchling_engine::inbox::{ActionId, Kind, State};
use serde_json::{json, Value};

fn go(rig: &Rig, confirmed: bool) -> Value {
    rig.promoter.start(confirmed).expect("an answer")
}

fn unreachable_primary(rig: &Rig) {
    let a = rig.a();
    rig.answer(&a, "node.load", Err(WireError::new("not_sent", "offline")));
}

fn claims(rig: &Rig) -> Vec<Claim> {
    rig.cluster
        .records
        .lock()
        .unwrap()
        .iter()
        .filter_map(|c| match c {
            Command::Claim { claim } => Some(claim.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn reachable_primary_answers_relayed() {
    let r = rig();
    let a = r.a();
    r.answer(&a, "node.load", Ok(json!({"cpu": 3})));
    let answer = go(&r, false);
    r.finish();
    assert_eq!(answer["relayed"], true, "{answer}");
    let text = answer["text"].as_str().unwrap();
    assert!(text.contains("m5 is reachable through m1"), "{text}");
    assert!(r.p.to.link.took.lock().unwrap().is_empty());
    assert_eq!(r.promoter.status()["step"], "relayed");
    assert!(logged(&r.p).is_empty());
}

#[test]
fn reached_through_a_peer_answers_relayed() {
    let r = rig();
    let c = crate::snap_testkit::id_of(3);
    r.add_peer(&c, "m2");
    r.answer(&c, "node.reach", Ok(json!({"online": true})));
    let answer = go(&r, false);
    r.finish();
    assert_eq!(answer["relayed"], true, "{answer}");
    let calls = r.cluster.calls.lock().unwrap().clone();
    assert!(calls.contains(&(c.clone(), "node.reach".to_string(), json!({"node": r.a()}))));
    assert!(calls
        .iter()
        .any(|(to, op, _)| *to == r.a() && op == "node.load"));
    assert!(r.p.to.link.took.lock().unwrap().is_empty());
}

#[test]
fn a_peer_that_also_cannot_reach_it_changes_nothing() {
    let r = rig();
    let c = crate::snap_testkit::id_of(3);
    r.add_peer(&c, "m2");
    r.answer(&c, "node.reach", Ok(json!({"online": false})));
    let answer = go(&r, false);
    r.finish();
    assert!(answer["needsConfirm"].is_string(), "{answer}");
}

#[test]
fn unreachable_primary_needs_confirm_and_moves_nothing() {
    let r = rig();
    unreachable_primary(&r);
    let answer = go(&r, false);
    r.finish();
    let text = answer["needsConfirm"].as_str().expect("a risk text");
    assert!(text.contains("m5 can't be reached"), "{text}");
    assert!(r.p.to.link.took.lock().unwrap().is_empty());
    let status = r.promoter.status();
    assert_eq!(status["step"], "confirm");
    assert_eq!(status["text"], text);
    assert_eq!(
        r.p.to
            .restore
            .restores
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[test]
fn confirmed_promote_takes_the_primary_and_resumes_the_session() {
    let r = rig();
    unreachable_primary(&r);
    let answer = go(&r, true);
    assert_eq!(answer, json!({"step": "bump"}));
    r.finish();
    let took = r.p.to.link.took.lock().unwrap();
    assert_eq!(took.len(), 1);
    assert_eq!(took[0].term, Term { n: 2, node: r.b() });
    assert_eq!(took[0].set.as_deref(), Some(r.p.manifest.set.as_str()));
    assert_eq!(took[0].sessions, vec![S1.to_string()]);
    assert!(took[0].staged && took[0].restored.is_some());
    drop(took);
    assert!(logged(&r.p)
        .iter()
        .any(|l| l.starts_with(&format!("to start {S1}"))));
    let status = r.promoter.status();
    assert_eq!(status["step"], "done", "{status}");
    assert_eq!(status["node"], json!(r.b()));
    assert!(status["started"].as_i64().unwrap() > 0);
    assert!(status.get("error").is_none());
}

#[test]
fn bump_is_journaled_before_any_session_starts() {
    let r = rig();
    unreachable_primary(&r);
    let log = r.p.log.clone();
    *r.p.to.link.on_take.lock().unwrap() = Some(Box::new(move |_| {
        log.lock().unwrap().push("take".to_string());
    }));
    go(&r, true);
    r.finish();
    let log = logged(&r.p);
    let at = |what: &str| log.iter().position(|l| l.starts_with(what)).expect(what);
    assert!(at("take") < at(&format!("claim {S1}")), "{log:?}");
    assert!(
        at(&format!("claim {S1}")) < at(&format!("to start {S1}")),
        "{log:?}"
    );
}

#[test]
fn a_failed_restore_changes_no_term() {
    let r = rig();
    unreachable_primary(&r);
    *r.p.to.restore.fail.lock().unwrap() = Some("the disk is full".to_string());
    let answer = go(&r, true);
    assert_eq!(answer, json!({"step": "bump"}));
    r.finish();
    assert!(r.p.to.link.took.lock().unwrap().is_empty());
    let status = Link::status(&*r.p.to.link);
    assert_eq!(status["cluster"]["home"], json!(r.a()));
    assert_eq!(status["cluster"]["term"]["n"], 1);
    assert!(logged(&r.p).is_empty());
    assert!(claims(&r).is_empty());
    let shown = r.promoter.status();
    assert_eq!(shown["step"], "failed");
    assert!(shown["error"]
        .as_str()
        .unwrap()
        .contains("the disk is full"));
    let item = r.item("promote-failed:2").expect("a notice");
    assert_eq!(item.kind, Kind::ActionFailed);
}

#[test]
fn a_failed_restore_check_changes_no_term() {
    let r = rig();
    unreachable_primary(&r);
    std::fs::write(r.p.w.at(".claude.json"), "{}").unwrap();
    go(&r, true);
    r.finish();
    assert!(r.p.to.link.took.lock().unwrap().is_empty());
    let shown = r.promoter.status();
    assert_eq!(shown["step"], "failed");
    assert!(
        shown["error"].as_str().unwrap().contains("isn't trusted"),
        "{shown}"
    );
}

#[test]
fn promote_without_proof_refuses_when_sessions_would_resume() {
    let r = rig_with(&one(), false);
    unreachable_primary(&r);
    let err = r.promoter.start(true).unwrap_err();
    assert_eq!(err.message, NO_PROOF);
    assert!(r.p.to.link.took.lock().unwrap().is_empty());
    assert!(!r.promoter.running());
    assert_eq!(r.promoter.status(), Value::Null);
}

fn drop_the_set(r: &Rig) {
    let root = r.p.w.cluster.join("standby").join(r.a().as_str());
    std::fs::remove_file(root.join("current.json")).unwrap();
}

#[test]
fn promote_with_no_set_controls_jobs_only() {
    let r = rig_with(&one(), false);
    drop_the_set(&r);
    unreachable_primary(&r);
    go(&r, true);
    r.finish();
    let took = r.p.to.link.took.lock().unwrap();
    assert_eq!(took.len(), 1);
    assert!(took[0].restored.is_none() && !took[0].staged);
    assert!(took[0].sessions.is_empty() && took[0].set.is_none());
    drop(took);
    assert_eq!(
        r.p.to
            .restore
            .restores
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert!(logged(&r.p).is_empty());
    assert!(claims(&r).is_empty());
    assert_eq!(r.promoter.status()["step"], "done");
}

#[test]
fn promote_with_a_set_of_unmovable_sessions_needs_no_proof_either() {
    let specs = [Spec {
        id: S2,
        flags: &["--name", "stay"],
        movable: false,
    }];
    let r = rig_with(&specs, false);
    unreachable_primary(&r);
    go(&r, true);
    r.finish();
    assert_eq!(r.p.to.link.took.lock().unwrap().len(), 1);
    assert!(logged(&r.p).is_empty());
}

#[test]
fn promote_claims_resumed_sessions_here() {
    let r = rig();
    unreachable_primary(&r);
    let previous = Claim {
        session: S1.to_string(),
        node: r.a(),
        n: 4,
        term: Term { n: 1, node: r.a() },
        base_set: "old".to_string(),
        base_len: 1,
        touched: true,
    };
    r.cluster
        .state
        .lock()
        .unwrap()
        .claims
        .insert(S1.to_string(), previous);
    go(&r, true);
    r.finish();
    let made = claims(&r);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].session, S1);
    assert_eq!(made[0].node, r.b());
    assert_eq!(made[0].n, 5);
    assert_eq!(made[0].term, Term { n: 2, node: r.b() });
    assert_eq!(made[0].base_set, r.p.manifest.set);
    assert_eq!(made[0].base_len, 8);
    assert!(!made[0].touched);
    let took = r.p.to.link.took.lock().unwrap();
    assert_eq!(
        took[0].claims.len(),
        1,
        "the takeover carries the old claim"
    );
}

#[test]
fn a_claim_that_could_not_be_recorded_starts_nothing() {
    let r = rig();
    unreachable_primary(&r);
    *r.cluster.record_fail.lock().unwrap() = Some("no home".to_string());
    go(&r, true);
    r.finish();
    assert!(logged(&r.p).is_empty(), "{:?}", logged(&r.p));
    let item = r.item(&format!("promote-resume:{S1}")).expect("a notice");
    assert_eq!(item.kind, Kind::ActionFailed);
    assert!(item.detail.contains("no home"));
    assert_eq!(r.promoter.status()["step"], "done");
}

fn cut_the_push(r: &Rig) {
    let path = r.p.w.at(&format!(".claude/projects/p/{S1}.jsonl"));
    *r.restore.after.lock().unwrap() = Some(Box::new(move || {
        std::fs::write(&path, CUT_PUSH).unwrap();
    }));
}

#[test]
fn cut_outward_step_raises_paused_step_item() {
    let r = rig();
    unreachable_primary(&r);
    cut_the_push(&r);
    go(&r, true);
    r.finish();
    let id = format!("paused-step:{S1}:0");
    let found = r.item(&id).expect("an item");
    assert_eq!(found.kind, Kind::PausedStep);
    assert_eq!(
        found.title,
        "m5 dropped while Push the notes branch was running. Check, then Continue."
    );
    assert!(found.detail.contains("outside this machine"));
    let acts: Vec<ActionId> = found.actions.iter().map(|a| a.id).collect();
    assert_eq!(acts, vec![ActionId::Continue, ActionId::Done]);
    assert_eq!(found.source.session.as_deref(), Some(S1));
    assert!(r.replier.sent.lock().unwrap().is_empty());
    assert_eq!(r.kinds(Kind::PausedStep).len(), 1);
}

#[test]
fn a_session_with_nothing_cut_raises_no_item() {
    let r = rig();
    unreachable_primary(&r);
    go(&r, true);
    r.finish();
    assert!(r.kinds(Kind::PausedStep).is_empty());
}

#[test]
fn continue_sends_one_reply() {
    let r = rig();
    unreachable_primary(&r);
    cut_the_push(&r);
    go(&r, true);
    r.finish();
    let id = format!("paused-step:{S1}:0");
    let outcome = r.p.to.inbox.decide(&id, ActionId::Continue, "mac").unwrap();
    assert!(outcome.answered);
    crate::cluster_testkit::eventually("the reply goes", || {
        !r.replier.sent.lock().unwrap().is_empty()
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(
        *r.replier.sent.lock().unwrap(),
        vec![(S1.to_string(), "Continue".to_string())]
    );
    assert_eq!(r.item(&id).unwrap().state, State::Answered);
    let again = r.p.to.inbox.decide(&id, ActionId::Continue, "mac").unwrap();
    assert!(!again.answered);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(r.replier.sent.lock().unwrap().len(), 1);
}

#[test]
fn continue_waits_while_acting_is_off() {
    let r = rig();
    unreachable_primary(&r);
    cut_the_push(&r);
    go(&r, true);
    r.finish();
    r.acting.set(false);
    let id = format!("paused-step:{S1}:0");
    let outcome =
        r.p.to
            .inbox
            .decide(&id, ActionId::Continue, "phone")
            .unwrap();
    assert!(!outcome.answered);
    assert_eq!(r.item(&id).unwrap().state, State::Open);
    assert!(r.replier.sent.lock().unwrap().is_empty());
}

#[test]
fn a_failed_continue_says_so() {
    let r = rig();
    unreachable_primary(&r);
    cut_the_push(&r);
    go(&r, true);
    r.finish();
    *r.replier.fail.lock().unwrap() = Some("acting is off".to_string());
    let id = format!("paused-step:{S1}:0");
    r.p.to.inbox.decide(&id, ActionId::Continue, "mac").unwrap();
    crate::cluster_testkit::eventually("the notice", || {
        r.item(&format!("paused-step-failed:{S1}")).is_some()
    });
}

#[test]
fn done_closes_paused_step() {
    let r = rig();
    unreachable_primary(&r);
    cut_the_push(&r);
    go(&r, true);
    r.finish();
    let id = format!("paused-step:{S1}:0");
    let outcome = r.p.to.inbox.decide(&id, ActionId::Done, "mac").unwrap();
    assert!(outcome.answered);
    assert_eq!(r.item(&id).unwrap().state, State::Answered);
    assert_eq!(r.item(&id).unwrap().answer, Some(ActionId::Done));
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(r.replier.sent.lock().unwrap().is_empty());
}

#[test]
fn failover_notice_when_done() {
    let r = rig();
    unreachable_primary(&r);
    go(&r, true);
    r.finish();
    let notice = r.item("failover:2").expect("a notice");
    assert_eq!(notice.kind, Kind::Failover);
    assert_eq!(notice.title, "m1 is primary now");
    assert!(notice.actions.is_empty());
}

#[test]
fn status_shows_the_promote() {
    let r = rig();
    assert_eq!(r.promoter.status(), Value::Null);
    unreachable_primary(&r);
    go(&r, false);
    r.finish();
    assert_eq!(r.promoter.status()["step"], "confirm");
    go(&r, true);
    r.finish();
    let status = r.promoter.status();
    assert_eq!(status["step"], "done");
    assert_eq!(status["node"], json!(r.b()));
    assert!(status["text"]
        .as_str()
        .unwrap()
        .contains("m1 is primary now"));
}

#[test]
fn a_second_promote_is_refused_while_one_runs() {
    let r = rig();
    unreachable_primary(&r);
    let gate = std::sync::Arc::new(std::sync::Mutex::new(()));
    let held = gate.lock().unwrap();
    let hold = gate.clone();
    *r.p.to.link.on_take.lock().unwrap() = Some(Box::new(move |_| {
        drop(hold.lock().unwrap());
    }));
    go(&r, true);
    assert!(r.promoter.running());
    let err = r.promoter.start(true).unwrap_err();
    assert!(err.message.contains("already running"), "{}", err.message);
    drop(held);
    r.finish();
}

#[test]
fn an_unrelated_item_is_left_alone() {
    let r = rig();
    unreachable_primary(&r);
    r.p.to.inbox.raise(item("other"), None);
    go(&r, true);
    r.finish();
    assert!(r.open_ids().contains(&"other".to_string()));
}

#[test]
fn a_promote_does_not_take_over_while_a_handover_switches() {
    let r = rig();
    unreachable_primary(&r);
    let held = r.promoter.p.mover.begin_switch(9).expect("the switch");
    go(&r, true);
    r.finish();
    assert!(r.p.to.link.took.lock().unwrap().is_empty());
    drop(held);
}

use crate::cluster_move::Link;
