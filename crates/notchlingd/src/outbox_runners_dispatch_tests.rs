use super::*;
use crate::outbox_courier::NOTHING;
use notchling_engine::outbox::{after, After};
use notchling_engine::tools::acp::fake::{FakeSpawner, Responder};
use notchling_engine::tools::agy::elsewhere::Holder;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};

type Load = Arc<Mutex<Option<Value>>>;

fn holding(load: Load, fail: Option<&'static str>) -> FakeSpawner {
    let responder: Responder = Arc::new(move |m, p| match m["method"].as_str() {
        Some("initialize") => {
            p.push(&json!({"jsonrpc": "2.0", "id": m["id"], "result": {"protocolVersion": 1}}))
        }
        Some("session/load") => match fail {
            Some(why) => p.push(&json!({"jsonrpc": "2.0", "id": m["id"],
                                        "error": {"code": -1, "message": why}})),
            None => *load.lock().unwrap() = Some(m.clone()),
        },
        _ => {}
    });
    FakeSpawner::with_responder(responder)
}

fn wait_for_load(load: &Load) -> Value {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(request) = load.lock().unwrap().clone() {
            return request;
        }
        assert!(Instant::now() < end, "session/load never came");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn agy_on(rig: agy_t::Rig) -> AgyBench {
    let b = bench(
        Arc::new(Acting::in_memory(true)),
        rig.room.clone(),
        None,
        Some(&rig.ops),
    );
    let courier = AgyCourier::new(rig.ops.clone());
    AgyBench { rig, b, courier }
}

fn agy_holding(fail: Option<&'static str>, wait: Duration) -> (AgyBench, Load) {
    let load: Load = Arc::new(Mutex::new(None));
    let rig = agy_t::rig_full(true, holding(load.clone(), fail), Some("/fake/agy"), wait);
    (agy_on(rig), load)
}

fn sending(b: &Bench, id: &str) -> Row {
    let row = b.row(id).expect("the row is kept");
    assert_eq!(row.state, State::Sending);
    row
}

#[test]
fn agy_load_failure_keeps_the_row() {
    let (a, _) = agy_holding(Some("no such conversation"), Duration::from_secs(10));
    let row = a.b.send(ROW, "hi");
    a.b.postmen.postman(&row.session).pass(a.b.at(T0));
    assert_eq!(a.b.state(&row.id), Some((State::Waiting, Some(Why::Retry))));
    let note = a.b.outbox.note(&row.id).unwrap();
    assert!(note.contains("no such conversation"), "{note}");
    assert!(a.b.removed().is_empty());
}

#[test]
fn agy_prompt_dispatched_only_then_sent() {
    let (a, load) = agy_holding(None, Duration::from_secs(10));
    let row = a.b.send(ROW, "hi");
    let mut p = a.b.postmen.postman(&row.session);
    let at = a.b.at(T0);
    let passing = std::thread::spawn(move || p.pass(at));
    let request = wait_for_load(&load);
    std::thread::sleep(Duration::from_millis(100));
    sending(&a.b, &row.id);
    assert!(a.b.removed().is_empty(), "not Sent while the load runs");
    let process = a.rig.process();
    assert!(process.requests("session/prompt").is_empty());
    process.push(&json!({"jsonrpc": "2.0", "id": request["id"], "result": {}}));
    passing.join().unwrap();
    assert_eq!(a.b.row(&row.id), None);
    assert_eq!(a.b.removed()[0]["why"], "sent");
    assert_eq!(process.requests("session/prompt").len(), 1);
}

#[test]
fn agy_still_queued_at_the_bound_is_unsure() {
    let (a, _) = agy_holding(None, Duration::from_millis(100));
    let row = a.b.send(ROW, "hi");
    a.b.postmen.postman(&row.session).pass(a.b.at(T0));
    assert_eq!(
        a.b.state(&row.id),
        Some((State::Unsure, Some(Why::Restart)))
    );
    assert!(a.b.removed().is_empty());
}

#[test]
fn agy_acting_off_before_dispatch_waits() {
    let (a, load) = agy_holding(None, Duration::from_secs(10));
    let row = a.b.send(ROW, "hi");
    let taken = a.b.outbox.store().take(&row.id, T0).unwrap().unwrap();
    let courier = AgyCourier::new(a.rig.ops.clone());
    let delivering = std::thread::spawn(move || courier.deliver(&taken, false));
    wait_for_load(&load);
    a.rig.acting.set(false);
    a.rig.ops.acting_off();
    let (end, tried, note) = delivering.join().unwrap();
    assert_eq!((end, tried, note), (TryEnd::NotIdle, NOTHING, None));
    assert!(a.rig.process().requests("session/prompt").is_empty());
}

#[test]
fn codex_acting_off_after_spawn_is_unsure() {
    let c = codex("cut");
    c.s.spawner.push(FakeRun::held(&[DONE]));
    let acting = c.s.acting.clone();
    c.s.spawner.on_spawn(Box::new(move || {
        acting.set(false);
    }));
    let row = c.b.send(&c.id, "go");
    c.b.postmen.postman(&row.session).pass(c.b.at(T0));
    assert_eq!(
        c.b.state(&row.id),
        Some((State::Unsure, Some(Why::Restart)))
    );
    assert!(c.b.removed().is_empty());
    assert_eq!(c.s.spawner.processes()[0].interrupts(), 1);
    c.s.idle();
}

#[test]
fn not_allowed_waits_without_note() {
    let c = codex("not-allowed");
    let row = c.b.send(&c.id, "hi");
    let taken = c.b.outbox.store().take(&row.id, T0).unwrap().unwrap();
    c.s.acting.set(false);
    let (end, tried, note) = c.courier.deliver(&taken, false);
    assert_eq!((&end, tried, &note), (&TryEnd::NotIdle, NOTHING, &None));
    assert_eq!(
        after(&end, tried, taken.tries, T0),
        After::Wait {
            why: Why::Turn,
            next_ms: None
        }
    );
    assert!(c.s.spawner.specs().is_empty());

    let a = agy_on(agy_t::rig(true));
    let row = a.b.send(ROW, "hi");
    let taken = a.b.outbox.store().take(&row.id, T0).unwrap().unwrap();
    a.rig.acting.set(false);
    let (end, tried, note) = a.courier.deliver(&taken, false);
    assert_eq!((end, tried, note), (TryEnd::NotIdle, NOTHING, None));
    assert!(a.rig.spawner.calls().is_empty());
}

#[test]
fn held_agy_session_is_not_taken_each_second() {
    let a = agy();
    let row = a.b.send(ROW, "hi");
    let finds = Arc::new(AtomicUsize::new(0));
    let counted = finds.clone();
    a.rig.ops.elsewhere().set_finder(
        Arc::new(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Some(Holder {
                pid: 4242,
                delegation: true,
            })
        }),
        Duration::from_secs(60),
    );
    let mut p = a.b.postmen.postman(&row.session);
    for tick in 0..3 {
        p.pass(a.b.at(T0 + tick * 1_000));
    }
    let kept = a.b.row(&row.id).unwrap();
    assert_eq!(
        (kept.state, kept.why, kept.tries),
        (State::Waiting, Some(Why::Turn), 0)
    );
    assert_eq!(finds.load(Ordering::SeqCst), 1, "the held state is cached");
    assert!(a.rig.spawner.calls().is_empty());
}
