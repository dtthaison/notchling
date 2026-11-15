use crate::cluster_move::{failed, resume_wait, trust_check, MoveGate};
use crate::cluster_move_calls::read_took;
use crate::cluster_move_run::{MoveRunner, SAGA_FILE};
use crate::cluster_move_tests::{
    logged, one, origin, pair, run, runner, runner_in, takeover, trusting, Fault, Log, Pair, Spec,
    S2, WAIT,
};
use crate::cluster_node::{api_error, refused, wire_error};
use crate::inbox_ops_tests::item;
use crate::snap_files_tests::{now_ms, put};
use crate::snap_restore_fs::staged_db;
use crate::snap_restore_testkit::S1;
use crate::snap_testkit::id_of;
use notchling_engine::cluster::move_home::{resume_call_ms, MoveSaga, MoveStep, IN_PROGRESS};
use notchling_engine::cluster::wire::WireError;
use notchling_engine::inbox::Kind;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

fn moved() -> Vec<String> {
    vec![
        format!("from stop {S1}"),
        format!("to start {S1} --name tester --model x"),
    ]
}

fn undone() -> Vec<String> {
    vec![
        format!("from stop {S1}"),
        format!("from start {S1} --name tester --model x"),
    ]
}

fn switch_to_b(p: &Pair, set: Option<&str>) -> Result<Value, notchling_local_api::ApiError> {
    let mut ask = takeover(&p.a, &p.b, 2, &[S1]);
    ask.set = set.map(str::to_string);
    p.to.mover
        .switch_here(&serde_json::to_value(&ask).unwrap(), &origin(&p.a))
}

fn reason(runner: &MoveRunner) -> String {
    runner.status()["reason"].as_str().unwrap_or("").to_string()
}

#[test]
fn h1_a_refusal_after_a_lost_answer_rolls_forward() {
    for code in ["not_sent", "refused", "stale_term"] {
        let p = pair(&one(), &|_| {});
        p.from
            .link
            .switch_faults
            .lock()
            .unwrap()
            .extend([Fault::LoseAnswer, Fault::Fail(code)]);
        let runner = run(&p);

        assert_eq!(
            runner.status()["step"],
            "done",
            "{code}: {}",
            runner.status()
        );
        assert_eq!(logged(&p), moved(), "{code}");
        assert_eq!(p.from.link.switches.load(SeqCst), 3, "{code}");
        assert_eq!(p.to.link.took.lock().unwrap().len(), 1, "{code}");
    }
}

#[test]
fn h1_a_restart_at_switch_with_no_state_yet_rolls_forward() {
    let p = pair(&one(), &|_| {});
    switch_to_b(&p, Some(&p.manifest.set)).expect("the switch landed");
    let mut saga = MoveSaga::new(p.a.clone(), p.b.clone(), now_ms(), false);
    saga.step = MoveStep::Switch;
    saga.term = 1;
    saga.to_name = "m1".to_string();
    saga.moving = vec![S1.to_string()];
    saga.stopped = vec![S1.to_string()];
    saga.set = Some(p.manifest.set.clone());
    let dir = p.w.at("journal");
    put(&dir.join(SAGA_FILE), &serde_json::to_vec(&saga).unwrap());
    *p.from.link.nulls.lock().unwrap() = 1;

    let runner = runner(&p);
    runner.recover();
    assert!(runner.wait_done(WAIT));

    assert_eq!(runner.status()["step"], "done", "{}", runner.status());
    assert_eq!(
        logged(&p),
        vec![format!("to start {S1} --name tester --model x")]
    );
    let fences = p.from.link.fences.lock().unwrap().clone();
    assert_eq!(
        fences.first(),
        Some(&Some("The primary is moving to m1".to_string()))
    );
    assert_eq!(fences.last(), Some(&None));
}

#[test]
fn h2_a_restore_refusal_undoes_once_and_is_not_retried() {
    let p = pair(&one(), &|_| {});
    *p.to.restore.fail.lock().unwrap() = Some("disk full".to_string());
    let runner = run(&p);

    assert_eq!(runner.status()["step"], "undone");
    assert_eq!(
        reason(&runner),
        "restoring from the snapshot failed: disk full"
    );
    assert_eq!(p.to.restore.restores.load(SeqCst), 1);
    assert_eq!(p.from.link.switches.load(SeqCst), 1);
    assert_eq!(logged(&p), undone());
    assert!(!staged_db(&p.to.cluster).exists());
    assert!(p.to.link.took.lock().unwrap().is_empty());
}

#[test]
fn h2_kept_codes_survive_the_local_api() {
    assert_eq!(wire_error(refused("no")).code, "refused");
    assert_eq!(wire_error(failed("no")).code, "failed");
    for code in ["refused", "not_sent", "stale_term"] {
        assert_eq!(wire_error(api_error(WireError::new(code, "x"))).code, code);
    }
    assert_eq!(
        api_error(WireError::new("stale_term", "x")).code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        wire_error(api_error(WireError::new("timeout", "x"))).code,
        "failed"
    );
}

#[test]
fn m1_each_stop_is_journaled_before_it_happens() {
    let p = pair(&one(), &|_| {});
    let journal = p.w.at("journal").join(SAGA_FILE);
    let seen: Log = Arc::default();
    let saw = seen.clone();
    *p.from.acts.on_stop.lock().unwrap() = Some(Box::new(move |id| {
        let saga: MoveSaga = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
        saw.lock()
            .unwrap()
            .push(format!("{id} {:?} {:?}", saga.stopping, saga.stopped));
    }));
    run(&p);

    assert_eq!(*seen.lock().unwrap(), vec![format!("{S1} [\"{S1}\"] []")]);
}

fn live(id: &str) -> Value {
    serde_json::to_value(Agent {
        id: id.to_string(),
        name: id.to_string(),
        project: "repo".to_string(),
        cwd: "/work".to_string(),
        group: AgentGroup::Completed,
        label: "x".to_string(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job("job-1".to_string()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".to_string(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
    })
    .unwrap()
}

fn both() -> Vec<Spec> {
    vec![
        Spec {
            id: S1,
            flags: &["--name", "one"],
            movable: true,
        },
        Spec {
            id: S2,
            flags: &["--name", "two"],
            movable: true,
        },
    ]
}

#[test]
fn m1_an_undo_restarts_a_stop_in_flight_only_if_it_is_down() {
    let p = pair(&both(), &|_| {});
    *p.from.rows.0.lock().unwrap() = vec![live(S1)];

    let rows = p.from.mover.restart(&[], &[S1.to_string(), S2.to_string()]);
    assert!(rows.iter().all(|(_, r)| r.is_ok()), "{rows:?}");
    assert_eq!(logged(&p), vec![format!("from start {S2} --name two")]);

    p.log.lock().unwrap().clear();
    p.from.mover.restart(&[S1.to_string()], &[]);
    assert_eq!(logged(&p), vec![format!("from start {S1} --name one")]);
}

#[test]
fn m2_a_lost_resume_answer_is_retried_without_a_false_failure() {
    let p = pair(&one(), &|_| {});
    p.from
        .link
        .call_faults
        .lock()
        .unwrap()
        .extend([Fault::Fail("timeout"), Fault::LoseAnswer]);
    let runner = run(&p);

    assert_eq!(runner.status()["step"], "done", "{}", runner.status());
    assert_eq!(logged(&p), moved());
    let failures: Vec<String> = p
        .from
        .inbox
        .items()
        .into_iter()
        .filter(|i| i.kind == Kind::ActionFailed)
        .map(|i| i.title)
        .collect();
    assert!(failures.is_empty(), "{failures:?}");
    let waits = p.from.link.waits.lock().unwrap().clone();
    assert_eq!(waits, vec![resume_wait(1); 3]);
    assert!(resume_wait(1) > Duration::from_millis(resume_call_ms(1)));
    assert!(resume_wait(40) > resume_wait(1));
}

#[test]
fn m3_a_target_on_another_notchling_is_refused_before_anything_moves() {
    let p = pair(&one(), &|_| {});
    p.from
        .link
        .edit(|s| s["members"][1]["versions"]["notchling"] = json!("0.0.0-other"));
    let runner = runner(&p);

    let err = runner.start(&p.b, false).unwrap_err();
    assert!(err.message.contains("0.0.0-other"), "{}", err.message);
    assert!(logged(&p).is_empty());

    p.from.link.edit(|s| {
        s["members"][1]["versions"] = s["members"][0]["versions"].clone();
    });
    runner
        .start(&p.b, false)
        .expect("a refused start leaves no move running");
    assert!(runner.wait_done(WAIT));
    assert_eq!(runner.status()["step"], "done");
}

#[test]
fn m4_only_the_previous_home_resumes_and_only_in_its_term() {
    let p = pair(&one(), &|_| {});
    let args = json!({"sessions": [S1]});
    let none = p.to.mover.resume_op(&args, &origin(&p.a)).unwrap_err();
    assert_eq!(none.message, "this machine took no handover");
    assert_eq!(wire_error(none).code, "refused");

    switch_to_b(&p, None).expect("the switch");
    let took = read_took(&p.to.cluster).expect("the took file");
    assert_eq!((took.term, &took.from), (2, &p.a));
    let stranger = p.to.mover.resume_op(&args, &origin(&id_of(3))).unwrap_err();
    assert_eq!(stranger.message, "only m1 can resume its sessions here");
    assert_eq!(wire_error(stranger).code, "refused");

    p.to.mover.resume_op(&args, &origin(&p.a)).expect("resume");
    let again = p.to.mover.resume_op(&args, &origin(&p.a)).expect("again");
    assert_eq!(again["results"][0]["ok"], true);
    assert_eq!(
        logged(&p),
        vec![format!("to start {S1} --name tester --model x")]
    );

    p.to.link.set_home(&id_of(3), 3);
    let over = p.to.mover.resume_op(&args, &origin(&p.a)).unwrap_err();
    assert_eq!(over.message, "that handover is over");
}

#[test]
fn m4_a_resume_skips_a_session_already_live() {
    let p = pair(&one(), &|_| {});
    switch_to_b(&p, None).expect("the switch");
    *p.to.rows.0.lock().unwrap() = vec![live(S1)];

    let answer =
        p.to.mover
            .resume_op(&json!({"sessions": [S1]}), &origin(&p.a))
            .expect("resume");
    assert_eq!(answer["results"][0]["ok"], true);
    assert!(logged(&p).is_empty());
}

#[test]
fn m5_a_second_switch_while_one_runs_is_in_progress_not_refused() {
    let p = pair(&one(), &|_| {});
    *p.to.mover.switching.lock().unwrap() = Some(2);

    let err = switch_to_b(&p, None).unwrap_err();
    assert_eq!(err.message, IN_PROGRESS);
    assert_eq!(wire_error(err).code, "failed");
    assert_eq!(p.to.restore.restores.load(SeqCst), 0);
}

#[test]
fn m5_a_takeover_that_fails_keeps_the_database_and_drops_the_stage() {
    let p = pair(&one(), &|_| {});
    let db = p.to.cluster.join("cluster.db");
    let before = std::fs::read(&db).unwrap();
    *p.to.link.take_fails.lock().unwrap() = Some("the core said no".to_string());

    let err = switch_to_b(&p, None).unwrap_err();
    assert_eq!(wire_error(err).code, "refused");
    assert_eq!(p.to.restore.restores.load(SeqCst), 1);
    assert_eq!(std::fs::read(&db).unwrap(), before);
    assert!(!staged_db(&p.to.cluster).exists());

    *p.to.link.take_fails.lock().unwrap() = None;
    switch_to_b(&p, None).expect("the switch");
    assert!(!staged_db(&p.to.cluster).exists());
    assert_eq!(p.to.link.took.lock().unwrap().len(), 1);
}

#[test]
fn m6_the_target_restores_exactly_the_final_set_or_refuses() {
    let p = pair(&one(), &|_| {});

    let err = switch_to_b(&p, Some("s-9")).unwrap_err();
    assert!(
        err.message
            .contains("doesn't hold the final snapshot s-9 complete"),
        "{}",
        err.message
    );
    assert_eq!(wire_error(err).code, "refused");
    assert_eq!(p.to.restore.restores.load(SeqCst), 0);
    assert!(read_took(&p.to.cluster).is_none());
}

#[test]
fn m6_no_newer_set_goes_to_the_target_until_the_move_ends() {
    let p = pair(&one(), &|_| {});
    run(&p);

    let holds = p.from.snaps.holds.lock().unwrap().clone();
    assert_eq!(holds.first(), Some(&(p.b.clone(), true)));
    assert_eq!(holds.last(), Some(&(p.b.clone(), false)));
    let took = p.to.link.took.lock().unwrap();
    assert_eq!(took[0].set.as_deref(), Some(p.manifest.set.as_str()));
}

#[test]
fn m7_the_gate_fences_new_work_while_the_primary_moves() {
    let gate = MoveGate::default();
    gate.fence("m1");
    for op in [
        ops::SESSION_NEW,
        ops::SESSION_MOVE,
        ops::CLUSTER_DISPATCH,
        ops::CLUSTER_MAIN_SET,
        ops::CLUSTER_RETURN,
    ] {
        let err = gate.check(op, &json!({}), &|_| None).unwrap_err();
        assert_eq!(err.message, "The primary is moving to m1", "{op}");
    }
    assert!(gate
        .check("session.reply", &json!({"id": "other"}), &|_| None)
        .is_ok());
    gate.unfence();
    assert!(gate.check(ops::SESSION_NEW, &json!({}), &|_| None).is_ok());
}

#[test]
fn m7_the_move_fences_the_core_and_names_what_was_raised_meanwhile() {
    let p = pair(&one(), &|_| {});
    let inbox = p.from.inbox.clone();
    *p.from.acts.on_stop.lock().unwrap() = Some(Box::new(move |_| {
        let mut late = item("late");
        late.created_ms = now_ms();
        inbox.raise(late, None);
    }));
    run(&p);

    let fences = p.from.link.fences.lock().unwrap().clone();
    assert_eq!(
        fences.first(),
        Some(&Some("The primary is moving to m1".to_string()))
    );
    assert_eq!(fences.last(), Some(&None));
    let details: Vec<String> = p
        .from
        .inbox
        .items()
        .into_iter()
        .filter(|i| i.kind == Kind::Notice)
        .map(|i| i.detail)
        .collect();
    assert!(
        details
            .iter()
            .any(|d| d == "Raised on this machine during the move, still here: title late"),
        "{details:?}"
    );
}

#[test]
fn l1_done_lifts_the_freeze() {
    let p = pair(&one(), &|_| {});
    run(&p);

    let gate = p.from.mover.gate();
    assert!(gate.frozen().is_empty());
    assert!(gate
        .check("session.reply", &json!({"id": S1}), &|_| None)
        .is_ok());
    assert!(gate.check(ops::SESSION_NEW, &json!({}), &|_| None).is_ok());
}

fn with_runner(
    p: &Pair,
    hook: impl Fn(&Arc<MoveRunner>) + Send + Sync + 'static,
) -> Arc<MoveRunner> {
    let runner = runner(p);
    let late: Arc<OnceLock<Arc<MoveRunner>>> = Arc::default();
    let _ = late.set(runner.clone());
    *p.from.acts.on_stop.lock().unwrap() = Some(Box::new(move |_| {
        if let Some(r) = late.get() {
            hook(r);
        }
    }));
    runner
}

#[test]
fn l2_a_new_move_forgets_an_old_nudge() {
    let p = pair(&one(), &|_| {});
    let runner = with_runner(&p, |r| {
        r.move_now().expect("a move is running");
    });
    runner.start(&p.b, false).expect("the move starts");
    assert!(runner.wait_done(WAIT));
    assert!(runner.nudge_pending(), "the late nudge outlived the move");

    let _ = runner.start(&p.b, false);
    assert!(!runner.nudge_pending());
}

#[test]
fn l3_a_second_start_while_one_runs_is_refused() {
    let p = pair(&one(), &|_| {});
    let tried: Log = Arc::default();
    let saw = tried.clone();
    let b = p.b.clone();
    let runner = with_runner(&p, move |r| {
        let err = r.start(&b, false).unwrap_err();
        saw.lock().unwrap().push(err.message);
    });
    runner.start(&p.b, false).expect("the move starts");
    assert!(runner.wait_done(WAIT));

    assert_eq!(*tried.lock().unwrap(), vec!["A move is already running"]);
    assert_eq!(runner.status()["step"], "done");
}

#[test]
fn l3_a_move_that_cant_be_journaled_is_undone_before_it_stops_anything() {
    let p = pair(&one(), &|_| {});
    let runner = runner_in(&p, p.w.at("missing/journal"));
    runner.start(&p.b, false).expect("the move starts");
    assert!(runner.wait_done(WAIT));

    assert_eq!(runner.status()["step"], "undone");
    assert!(
        reason(&runner).starts_with("the move couldn't be saved"),
        "{}",
        reason(&runner)
    );
    assert!(logged(&p).is_empty());
    assert_eq!(p.from.link.switches.load(SeqCst), 0);
}

#[test]
fn l6_an_untrusted_folder_is_refused_before_the_takeover() {
    let p = pair(&one(), &|_| {});
    put(&p.w.at(".claude.json"), b"{}");
    let runner = run(&p);

    assert_eq!(runner.status()["step"], "undone");
    assert_eq!(
        reason(&runner),
        format!(
            "{} isn't trusted by Claude Code on mine",
            p.w.repo.display()
        )
    );
    assert_eq!(p.to.restore.restores.load(SeqCst), 0);
    assert_eq!(logged(&p), undone());
}

#[test]
fn l6_a_lane_session_is_checked_against_the_lane() {
    let p = pair(&one(), &|_| {});
    let lane = p.w.at(".claude-lanes/work");
    let mut manifest = p.manifest.clone();
    manifest.sessions[0].transcripts = vec![lane.join(format!("projects/p/{S1}.jsonl"))];
    let ids = BTreeSet::from([S1.to_string()]);

    let err = trust_check(&p.w.home, &manifest, &ids, "m1").unwrap_err();
    assert_eq!(
        err,
        format!("{} isn't trusted by Claude Code on m1", p.w.repo.display())
    );
    put(&lane.join(".claude.json"), trusting(&p.w.home).as_bytes());
    assert_eq!(trust_check(&p.w.home, &manifest, &ids, "m1"), Ok(()));
}

struct Quiet;

impl crate::cluster_relay::Wire for Quiet {
    fn status(&self) -> Value {
        json!({"cluster": null, "members": []})
    }

    fn call(
        &self,
        _: &notchling_engine::cluster::NodeId,
        _: &str,
        _: Value,
        _: Duration,
    ) -> Result<Value, WireError> {
        Err(WireError::new("offline", "nobody"))
    }
}

impl crate::cluster_relay::SetView for Quiet {
    fn newest_ms(&self) -> Option<i64> {
        None
    }

    fn transcript(&self, _: &std::path::Path) -> Option<std::path::PathBuf> {
        None
    }
}

fn held_relay(dir: &std::path::Path) -> Arc<crate::cluster_relay::HeldRelay> {
    Arc::new(crate::cluster_relay::HeldRelay::new(
        crate::cluster_relay::Parts {
            wire: Arc::new(Quiet),
            dir: dir.to_path_buf(),
            clock: Arc::new(|| 0),
            prompts: Arc::new(|_| None),
            model: Arc::new(|_| None),
            rows: Arc::new(Vec::new),
            set: Arc::new(Quiet),
            bridge: Arc::new(|| crate::cluster_relay::Served {
                port: 1,
                fingerprint: None,
            }),
            limits: crate::cluster_relay::Limits::DEFAULT,
        },
    ))
}

#[test]
fn r122_a_held_entry_survives_the_database_the_takeover_swaps_in() {
    use notchling_engine::cluster::held::{Held, HeldQueue, HeldTarget};
    use notchling_engine::cluster::persist;
    let p = pair(&one(), &|_| {});
    let mut queue = HeldQueue::default();
    queue.hold(Held {
        id: "h1".into(),
        op: "session.message".into(),
        args: json!({"sessionId": "x/y", "text": "wait for me"}),
        target: HeldTarget::Session("x/y".into()),
        device: "phone-1".into(),
        request: "4".into(),
        at_ms: 5,
    });
    persist::save_held(&p.to.cluster, &queue).expect("saved");
    assert!(p.to.acts.held.set(held_relay(&p.to.cluster)).is_ok());

    switch_to_b(&p, Some(&p.manifest.set)).expect("the switch landed");

    assert_eq!(p.to.acts.swaps.load(SeqCst), 1);
    let kept = persist::load_held(&p.to.cluster).expect("readable").list();
    assert_eq!(kept.len(), 1, "carried over the swap");
    assert_eq!(kept[0].id, "h1");
}

#[test]
fn r122_a_refused_takeover_leaves_them_where_they_were() {
    use notchling_engine::cluster::held::{Held, HeldQueue, HeldTarget};
    use notchling_engine::cluster::persist;
    let p = pair(&one(), &|_| {});
    let mut queue = HeldQueue::default();
    queue.hold(Held {
        id: "h1".into(),
        op: "session.sendNow".into(),
        args: json!({"sessionId": "x/y"}),
        target: HeldTarget::Session("x/y".into()),
        device: "phone-1".into(),
        request: "4".into(),
        at_ms: 5,
    });
    persist::save_held(&p.to.cluster, &queue).expect("saved");
    assert!(p.to.acts.held.set(held_relay(&p.to.cluster)).is_ok());
    *p.to.link.take_fails.lock().unwrap() = Some("the core said no".to_string());

    assert!(switch_to_b(&p, None).is_err());

    assert_eq!(
        persist::load_held(&p.to.cluster)
            .expect("readable")
            .list()
            .len(),
        1
    );
}
