use super::tests::{setup, Setup, SESSION};
use super::*;
use crate::cluster_fence::write_fence;
use crate::codex_fence::CodexFence;
use notchling_engine::cluster::fence::{
    after_wait, Fence, FenceFile, Gate, FENCE_VERSION, POLL_MS, WAIT_MAX_MS,
};
use notchling_engine::cluster::id::fingerprint;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::NodeId;
use notchling_engine::tools::codex::fake::FakeRun;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

const BOOT: &str = "boot-x";

fn node() -> NodeId {
    NodeId::from_fingerprint(&fingerprint(b"m5")).expect("a node id")
}

fn write(s: &Setup, valid_until: u64, gates: &[(&str, Gate)]) {
    let fence = FenceFile {
        v: FENCE_VERSION,
        term: Term { n: 1, node: node() },
        node: node(),
        boot_id: BOOT.to_string(),
        valid_until,
        sessions: gates
            .iter()
            .map(|(key, gate)| (key.to_string(), gate.clone()))
            .collect::<BTreeMap<_, _>>(),
    };
    write_fence(&s.dir.0.join("cluster"), &fence).expect("fence");
}

struct Clock {
    now: AtomicU64,
    slept: Mutex<Vec<u64>>,
}

fn gated(s: &Setup, now: u64) -> Arc<Clock> {
    let clock = Arc::new(Clock {
        now: AtomicU64::new(now),
        slept: Mutex::new(Vec::new()),
    });
    let (read, nap) = (clock.clone(), clock.clone());
    s.ops
        .set_fence(CodexFence::new(&s.dir.0.join("cluster"), BOOT).with_clock(
            Box::new(move || read.now.load(Ordering::SeqCst)),
            Box::new(move |ms| {
                nap.slept.lock().unwrap().push(ms);
                nap.now.fetch_add(ms, Ordering::SeqCst);
            }),
        ));
    clock
}

fn row() -> String {
    format!("codex:{SESSION}")
}

fn send(s: &Setup) -> Result<Value, ApiError> {
    s.call(ops::CODEX_SEND, json!({"id": row(), "text": "go on"}))
}

fn refusal(result: Result<Value, ApiError>) -> String {
    let error = result.expect_err("a refusal");
    assert_eq!(error.code, ErrorCode::Failed);
    error.message
}

#[test]
fn codex_send_on_expired_fence_is_refused() {
    let s = setup("fence-expired", true);
    write(&s, 5, &[(&row(), Gate::Lease)]);
    let clock = gated(&s, 1_000);
    let Fence::Deny(text) = after_wait() else {
        panic!("after_wait denies");
    };
    assert_eq!(refusal(send(&s)), text);
    assert!(s.spawner.specs().is_empty(), "nothing was started");
    let slept = clock.slept.lock().unwrap();
    assert!(!slept.is_empty() && slept.iter().all(|ms| *ms == POLL_MS));
    assert!(slept.iter().sum::<u64>() <= WAIT_MAX_MS);
}

#[test]
fn a_moved_codex_session_is_refused_at_once_naming_the_machine() {
    let s = setup("fence-moved", true);
    write(&s, u64::MAX, &[(&row(), Gate::Moved { to: "m1".into() })]);
    let clock = gated(&s, 1_000);
    let text = refusal(send(&s));
    assert!(text.contains("continues on m1"), "{text}");
    assert!(clock.slept.lock().unwrap().is_empty());
    assert!(s.spawner.specs().is_empty());
}

#[test]
fn the_bare_thread_id_is_gated_too() {
    let s = setup("fence-thread", true);
    write(&s, u64::MAX, &[(SESSION, Gate::Moved { to: "m1".into() })]);
    gated(&s, 1_000);
    let text = refusal(send(&s));
    assert!(text.contains("continues on m1"), "{text}");
}

#[test]
fn a_waiting_codex_session_says_what_it_waits_for() {
    let s = setup("fence-waiting", true);
    write(
        &s,
        u64::MAX,
        &[(
            &row(),
            Gate::Waiting {
                for_node: "m1".into(),
            },
        )],
    );
    gated(&s, 1_000);
    let text = refusal(send(&s));
    assert!(text.contains("waits for m1"), "{text}");
}

#[test]
fn a_valid_lease_and_an_ungated_session_send() {
    let s = setup("fence-pass", true);
    write(&s, u64::MAX, &[(&row(), Gate::Lease)]);
    gated(&s, 1_000);
    s.spawner.push(FakeRun::ok(&[]));
    assert!(send(&s).is_ok());
    s.idle();
    write(&s, 5, &[("another-session", Gate::Lease)]);
    s.spawner.push(FakeRun::ok(&[]));
    assert!(send(&s).is_ok());
    s.idle();
}

#[test]
fn a_lease_that_arrives_while_waiting_lets_the_message_through() {
    let s = setup("fence-arrives", true);
    write(&s, 5, &[(&row(), Gate::Lease)]);
    let clock = Arc::new(Clock {
        now: AtomicU64::new(1_000),
        slept: Mutex::new(Vec::new()),
    });
    let again = s.dir.0.join("cluster");
    let fence = FenceFile {
        v: FENCE_VERSION,
        term: Term { n: 1, node: node() },
        node: node(),
        boot_id: BOOT.to_string(),
        valid_until: u64::MAX,
        sessions: BTreeMap::from([(row(), Gate::Lease)]),
    };
    let (read, nap) = (clock.clone(), clock.clone());
    s.ops.set_fence(CodexFence::new(&again, BOOT).with_clock(
        Box::new(move || read.now.load(Ordering::SeqCst)),
        Box::new(move |ms| {
            nap.now.fetch_add(ms, Ordering::SeqCst);
            if nap.now.load(Ordering::SeqCst) >= 2_000 {
                write_fence(&again, &fence).unwrap();
            }
        }),
    ));
    s.spawner.push(FakeRun::ok(&[]));
    assert!(send(&s).is_ok());
    s.idle();
}

#[test]
fn a_damaged_fence_file_pauses_a_codex_send() {
    let s = setup("fence-damaged", true);
    std::fs::create_dir_all(s.dir.0.join("cluster")).unwrap();
    std::fs::write(s.dir.0.join("cluster/fence.json"), b"garbage").unwrap();
    gated(&s, 1_000);
    assert!(refusal(send(&s)).contains("paused"));
    assert!(s.spawner.specs().is_empty());
}
