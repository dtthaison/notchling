use crate::cluster_fence::read_fence;
use crate::cluster_fence_keeper_kit::{claim, mains, node, world, FakeCodex, Seed, World, S1, S2};
use notchling_engine::cluster::failover::Failover;
use notchling_engine::cluster::fence::Gate;
use notchling_engine::cluster::Tool;
use std::sync::{Arc, Mutex};

fn claimed_away() -> Seed {
    Seed {
        main: mains(&[(S1, Tool::Claude), (S2, Tool::Claude)]),
        claims: vec![claim(S1, "m1", 1)],
        ..Seed::default()
    }
}

fn live(w: &World, ids: &[&str]) {
    let mut set = w.feed.live.lock().unwrap();
    set.clear();
    set.extend(ids.iter().map(|id| id.to_string()));
}

fn stops(w: &World) -> Vec<String> {
    w.acts.stops.lock().unwrap().clone()
}

#[test]
fn step_down_writes_moved_then_stops() {
    let w = world("step-down", claimed_away());
    live(&w, &[S1, S2]);
    let seen: Arc<Mutex<Vec<Option<Gate>>>> = Arc::default();
    let (cluster, log) = (w.cluster.clone(), seen.clone());
    *w.acts.on_stop.lock().unwrap() = Some(Box::new(move |id: &str| {
        let gate = read_fence(&cluster).and_then(|f| f.sessions.get(id).cloned());
        log.lock().unwrap().push(gate);
    }));
    w.keeper.tick();
    assert_eq!(stops(&w), vec![format!("claude {S1}")]);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![Some(Gate::Moved { to: "m1".into() })],
        "the file held the Moved gate when the stop happened"
    );
}

#[test]
fn a_session_not_live_here_is_not_stopped() {
    let w = world("not-live", claimed_away());
    live(&w, &[S2]);
    w.keeper.tick();
    assert!(stops(&w).is_empty());
    assert_eq!(w.gate(S1), Some(Gate::Moved { to: "m1".into() }));
}

#[test]
fn a_session_is_stopped_once_per_claim() {
    let w = world("once", claimed_away());
    live(&w, &[S1]);
    for _ in 0..4 {
        w.keeper.tick();
    }
    assert_eq!(stops(&w).len(), 1);
    w.reseed(&Seed {
        claims: vec![claim(S1, "m1", 2)],
        ..claimed_away()
    });
    w.keeper.tick();
    assert_eq!(stops(&w).len(), 2, "a newer claim is a new stop");
    w.reseed(&Seed {
        claims: vec![{
            let mut other = claim(S1, "m1", 2);
            other.node = node("m2");
            other
        }],
        ..claimed_away()
    });
    w.keeper.tick();
    assert_eq!(stops(&w).len(), 3, "another owner is a new stop");
}

#[test]
fn a_failed_fence_write_still_stops_the_session() {
    let w = world("write-fails", claimed_away());
    live(&w, &[S1]);
    std::fs::write(w.path(), b"garbage").unwrap();
    w.keeper.tick();
    assert_eq!(stops(&w), vec![format!("claude {S1}")]);
    assert!(w.notice("cluster-fence-write").is_some());
}

#[test]
fn a_stop_that_fails_is_reported_and_retried_a_few_times() {
    let w = world("stop-fails", claimed_away());
    live(&w, &[S1]);
    *w.acts.fail.lock().unwrap() = true;
    for _ in 0..6 {
        w.keeper.tick();
    }
    assert_eq!(stops(&w).len(), 3);
    let text = w
        .notice(&format!("cluster-fence-stop-{S1}"))
        .expect("a notice");
    assert!(text.contains("claude stop failed"), "{text}");
}

#[test]
fn a_claim_naming_this_machine_stops_nothing() {
    let w = world(
        "mine",
        Seed {
            claims: vec![claim(S1, "m5", 1)],
            ..claimed_away()
        },
    );
    live(&w, &[S1]);
    w.keeper.tick();
    assert!(stops(&w).is_empty());
}

#[test]
fn a_codex_session_claimed_elsewhere_is_stopped_through_the_acts() {
    let id = "codex:019a5c3e-aaaa-7bbb-8ccc-000000000001";
    let w = world(
        "codex-step-down",
        Seed {
            main: mains(&[(id, Tool::Codex)]),
            claims: vec![claim(id, "m1", 1)],
            ..Seed::default()
        },
    );
    let codex = Arc::new(FakeCodex::running(&[
        "019a5c3e-aaaa-7bbb-8ccc-000000000001",
    ]));
    w.keeper.set_codex(codex.clone());
    w.keeper.tick();
    assert_eq!(stops(&w), vec![format!("codex {id}")]);
    w.keeper.tick();
    assert_eq!(stops(&w).len(), 1, "once per claim");
}

#[test]
fn expired_fence_interrupts_codex_turn() {
    use crate::codex_ops::tests::{setup, SESSION};
    use notchling_engine::tools::codex::fake::FakeRun;
    use serde_json::json;
    let s = setup("keeper-codex", true);
    let id = format!("codex:{SESSION}");
    let w = world(
        "codex-expired",
        Seed {
            failover: Failover::After30m,
            holder: true,
            main: mains(&[(&id, Tool::Codex)]),
            ..Seed::default()
        },
    );
    w.keeper.set_codex(s.ops.clone());
    s.spawner.push(FakeRun::held(&[]));
    s.ops
        .call(
            notchling_local_api::ops::CODEX_SEND,
            &json!({"id": id, "text": "go"}),
        )
        .expect("sent");
    assert_eq!(s.spawner.processes()[0].interrupts(), 0);
    *w.feed.latest.lock().unwrap() = None;
    w.keeper.tick();
    assert_eq!(
        s.spawner.processes()[0].interrupts(),
        1,
        "the Lease gate has valid_until 0 until a lease arrives"
    );
}

#[test]
fn a_valid_lease_leaves_a_codex_turn_running() {
    use crate::codex_ops::tests::{setup, SESSION};
    use notchling_engine::tools::codex::fake::FakeRun;
    use serde_json::json;
    let s = setup("keeper-codex-ok", true);
    let id = format!("codex:{SESSION}");
    let w = world(
        "codex-ok",
        Seed {
            main: mains(&[(&id, Tool::Codex)]),
            ..Seed::default()
        },
    );
    w.keeper.set_codex(s.ops.clone());
    s.spawner.push(FakeRun::held(&[]));
    s.ops
        .call(
            notchling_local_api::ops::CODEX_SEND,
            &json!({"id": id, "text": "go"}),
        )
        .expect("sent");
    w.keeper.tick();
    assert_eq!(s.spawner.processes()[0].interrupts(), 0);
}

#[test]
fn a_moved_codex_turn_is_interrupted() {
    use crate::codex_ops::tests::{setup, SESSION};
    use notchling_engine::tools::codex::fake::FakeRun;
    use serde_json::json;
    let s = setup("keeper-codex-moved", true);
    let id = format!("codex:{SESSION}");
    let w = world(
        "codex-moved",
        Seed {
            main: mains(&[(&id, Tool::Codex)]),
            claims: vec![claim(&id, "m1", 1)],
            ..Seed::default()
        },
    );
    w.keeper.set_codex(s.ops.clone());
    s.spawner.push(FakeRun::held(&[]));
    s.ops
        .call(
            notchling_local_api::ops::CODEX_SEND,
            &json!({"id": id, "text": "go"}),
        )
        .expect("sent");
    w.keeper.tick();
    assert_eq!(s.spawner.processes()[0].interrupts(), 1);
}
