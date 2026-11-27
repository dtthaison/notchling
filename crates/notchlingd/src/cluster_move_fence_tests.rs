use crate::cluster_fence::{fence_path, read_fence};
use crate::cluster_move_tests::{one, pair, run, Fault, Pair, WAIT};
use crate::hook_fence::answer;
use crate::snap_restore_testkit::S1;
use notchling_engine::cluster::fence::{Gate, FENCE_FILE};
use serde_json::{json, Value};
use std::cell::Cell;

fn hook(p: &Pair, session: &str) -> Option<Value> {
    let data = p.w.at("hook-data");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(data.join("cluster")).unwrap();
    if let Ok(bytes) = std::fs::read(fence_path(&p.from.cluster)) {
        std::fs::write(data.join(FENCE_FILE), bytes).unwrap();
    }
    let event = json!({"session_id": session, "hook_event_name": "PreToolUse"}).to_string();
    let now = Cell::new(1_000u64);
    let reply = answer(event.as_bytes(), &data, "boot-now", &|| now.get(), &|ms| {
        now.set(now.get() + ms)
    })?;
    serde_json::from_str(&reply).ok()
}

fn reason(reply: &Value) -> String {
    reply["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn r176_after_a_handover_the_old_primarys_session_is_denied_by_the_fence_hook() {
    let p = pair(&one(), &|_| {});
    assert!(hook(&p, S1).is_none());
    let runner = run(&p);
    assert_eq!(runner.status()["step"], "done");
    let held = read_fence(&p.from.cluster).expect("the old primary wrote a fence file");
    assert_eq!(held.sessions[S1], Gate::Moved { to: "m1".into() });
    assert_eq!(held.term.n, 2);
    assert_eq!(held.node, p.a);
    let reply = hook(&p, S1).expect("a deny");
    assert_eq!(reply["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(reason(&reply).contains("continues on m1"), "{reply}");
    assert!(hook(&p, "another-session").is_none());
}

#[test]
fn the_new_primary_writes_no_gate_for_the_session_it_runs() {
    let p = pair(&one(), &|_| {});
    run(&p);
    assert!(read_fence(&p.to.cluster).is_none());
}

#[test]
fn a_move_that_never_switched_leaves_the_session_passing() {
    let p = pair(&one(), &|_| {});
    for _ in 0..8 {
        p.from
            .link
            .switch_faults
            .lock()
            .unwrap()
            .push_back(Fault::Fail("refused"));
    }
    let runner = crate::cluster_move_tests::runner(&p);
    runner.start(&p.b, false).expect("the move starts");
    assert!(runner.wait_done(WAIT));
    assert_ne!(runner.status()["step"], "done");
    assert!(read_fence(&p.from.cluster).is_none());
    assert!(hook(&p, S1).is_none());
}

#[test]
fn a_switch_whose_answer_was_lost_still_fences() {
    let p = pair(&one(), &|_| {});
    p.from
        .link
        .switch_faults
        .lock()
        .unwrap()
        .push_back(Fault::LoseAnswer);
    let runner = run(&p);
    assert_eq!(runner.status()["step"], "done");
    let reply = hook(&p, S1).expect("a deny");
    assert!(reason(&reply).contains("continues on m1"), "{reply}");
}

#[test]
fn starting_the_session_here_again_lifts_its_gate() {
    let p = pair(&one(), &|_| {});
    run(&p);
    assert!(hook(&p, S1).is_some());
    let rows = p.from.mover.start_here(&[S1.to_string()]);
    assert!(rows[0].1.is_ok());
    assert!(hook(&p, S1).is_none());
}
