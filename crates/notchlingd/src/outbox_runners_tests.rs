use crate::acting::Acting;
use crate::acting_ops_tests::{app, code, FakeRoom};
use crate::agy_ops::tests::{self as agy_t, ROW};
use crate::agy_ops::{AgyOps, CANCEL, PERMISSION};
use crate::codex_ops::tests::{self as codex_t, SESSION as CODEX_SESSION};
use crate::codex_ops::CodexOps;
use crate::op_group::OpGroup;
use crate::outbox_courier::{Courier, Refuse};
use crate::outbox_ops::{OutboxOps, SendArgs};
use crate::outbox_postman::{Couriers, Next, Postmen};
use crate::outbox_runners::{link_agy, link_codex, AgyCourier, CodexCourier};
use crate::outbox_testkit::Script;
use notchling_engine::outbox::{OutboxStore, Row, State, TryEnd, Via, Why};
use notchling_engine::sessions::Roots;
use notchling_engine::tools::codex::fake::FakeRun;
use notchling_engine::tools::codex::plan::Pick;
use notchling_engine::tools::codex::runs::Request;
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

const T0: i64 = 1_794_697_060_000;
const DONE: &str = r#"{"type":"turn.completed","usage":{}}"#;

struct Bench {
    outbox: Arc<OutboxOps>,
    postmen: Postmen,
    clock: Arc<AtomicI64>,
    verifier: Arc<Script>,
    room: Arc<FakeRoom>,
}

fn bench(
    acting: Arc<Acting>,
    room: Arc<FakeRoom>,
    codex: Option<&Arc<CodexOps>>,
    agy: Option<&Arc<AgyOps>>,
) -> Bench {
    let roots = Roots {
        main_dir: PathBuf::from("/nonexistent/claude"),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let outbox = Arc::new(OutboxOps::new(
        Arc::new(OutboxStore::in_memory_for_tests()),
        Arc::new(Mutex::new(json!([]))),
        roots,
        acting.clone(),
        room.clone(),
        None,
    ));
    let codex_courier: Arc<dyn Courier> = match codex {
        Some(codex) => {
            link_codex(&outbox, codex);
            Arc::new(CodexCourier::new(codex.clone()))
        }
        None => Arc::new(Refuse),
    };
    let agy_courier: Arc<dyn Courier> = match agy {
        Some(agy) => {
            link_agy(&outbox, agy);
            Arc::new(AgyCourier::new(agy.clone()))
        }
        None => Arc::new(Refuse),
    };
    let clock = Arc::new(AtomicI64::new(T0));
    let ticks = clock.clone();
    let verifier = Arc::new(Script::default());
    let postmen = Postmen::build(
        outbox.clone(),
        Couriers {
            claude: Arc::new(Refuse),
            codex: codex_courier,
            antigravity: agy_courier,
        },
        verifier.clone(),
        acting,
        Arc::new(move || ticks.load(Ordering::SeqCst)),
    );
    Bench {
        outbox,
        postmen,
        clock,
        verifier,
        room,
    }
}

impl Bench {
    fn at(&self, ms: i64) -> i64 {
        self.clock.store(ms, Ordering::SeqCst);
        ms
    }

    fn send(&self, id: &str, text: &str) -> Row {
        self.send_with(id, text, None)
    }

    fn send_with(&self, id: &str, text: &str, options: Option<Value>) -> Row {
        self.outbox
            .send(&SendArgs {
                id: id.into(),
                text: text.into(),
                clear_box: false,
                options,
                via: Via::Chat,
            })
            .unwrap()
    }

    fn row(&self, id: &str) -> Option<Row> {
        self.outbox.store().get(id).unwrap()
    }

    fn state(&self, id: &str) -> Option<(State, Option<Why>)> {
        self.row(id).map(|row| (row.state, row.why))
    }

    fn removed(&self) -> Vec<Value> {
        self.room.named("outbox.removed")
    }
}

struct CodexBench {
    s: codex_t::Setup,
    b: Bench,
    courier: CodexCourier,
    id: String,
}

fn codex(tag: &str) -> CodexBench {
    let s = codex_t::setup(&format!("runners-{tag}"), true);
    let b = bench(s.acting.clone(), s.room.clone(), Some(&s.ops), None);
    let courier = CodexCourier::new(s.ops.clone());
    CodexBench {
        s,
        b,
        courier,
        id: format!("codex:{CODEX_SESSION}"),
    }
}

impl CodexBench {
    fn running_turn(&self) {
        self.s.spawner.push(FakeRun::held(&[DONE]));
        self.s
            .ops
            .runs()
            .send(Request {
                text: "first".into(),
                session: Some(CODEX_SESSION.into()),
                folder: self.s.work(),
                pick: Pick::default(),
            })
            .unwrap();
    }

    fn finish_turn(&self) {
        self.s.spawner.processes()[0].release();
        self.s.idle();
    }
}

#[test]
fn busy_codex_waits_turn_then_starts() {
    let c = codex("busy");
    c.running_turn();
    c.s.spawner.push(FakeRun::ok(&[DONE]));
    let row = c.b.send(&c.id, "hi there");
    let mut p = c.b.postmen.postman(&row.session);
    assert_eq!(p.pass(c.b.at(T0)), Next::At(T0 + 1_000));
    assert_eq!(c.b.state(&row.id), Some((State::Waiting, Some(Why::Turn))));
    assert_eq!(c.s.spawner.specs().len(), 1);
    c.finish_turn();
    p.pass(c.b.at(T0 + 1_000));
    assert_eq!(c.b.row(&row.id), None);
    let specs = c.s.spawner.specs();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[1].args.last().map(String::as_str), Some("hi there"));
    assert_eq!(c.s.ops.runs().waiting(CODEX_SESSION), 0);
    c.s.idle();
}

#[test]
fn codex_race_to_busy_waits_turn() {
    let c = codex("race");
    let row = c.b.send(&c.id, "hi");
    let taken = c.b.outbox.store().take(&row.id, T0).unwrap().unwrap();
    c.running_turn();
    let (end, tried, note) = c.courier.deliver(&taken, false);
    assert_eq!(end, TryEnd::NotIdle);
    assert!(!tried.pasted && !tried.entered);
    assert_eq!(note, None);
    assert_eq!(c.s.ops.runs().waiting(CODEX_SESSION), 0);
    assert_eq!(c.s.spawner.specs().len(), 1);
    c.finish_turn();
}

#[test]
fn codex_spawn_error_retries_with_error_on_row() {
    let c = codex("spawn");
    c.s.spawner.push_error("boom");
    let row = c.b.send(&c.id, "hi");
    let mut p = c.b.postmen.postman(&row.session);
    p.pass(c.b.at(T0));
    assert_eq!(c.b.state(&row.id), Some((State::Waiting, Some(Why::Retry))));
    let note = c.b.outbox.note(&row.id).unwrap();
    assert!(note.contains("boom"), "{note}");
    assert!(!note.contains("hi"), "the message is not in the note");
    assert!(c.b.row(&row.id).unwrap().next_ms.unwrap() > T0);
}

#[test]
fn runner_started_is_sent_without_transcript_codex() {
    let c = codex("sent");
    c.s.spawner.push(FakeRun::ok(&[DONE]));
    let row = c.b.send(&c.id, "go");
    c.b.postmen.postman(&row.session).pass(c.b.at(T0));
    assert_eq!(c.b.row(&row.id), None);
    assert_eq!(c.b.removed().len(), 1);
    assert_eq!(c.b.removed()[0]["why"], "sent");
    assert_eq!(c.b.verifier.asked.load(Ordering::SeqCst), 0);
    assert!(!c.courier.verifies());
    c.s.idle();
}

#[test]
fn codex_options_are_checked_again_at_delivery() {
    let c = codex("options");
    let row = c.b.send_with(&c.id, "hi", Some(json!({"model": "a b"})));
    let mut p = c.b.postmen.postman(&row.session);
    p.pass(c.b.at(T0));
    assert_eq!(c.b.state(&row.id), Some((State::Waiting, Some(Why::Retry))));
    assert!(c.s.spawner.specs().is_empty());
    assert!(c.b.outbox.note(&row.id).unwrap().contains("model"));
}

#[test]
fn codex_options_reach_the_turn() {
    let c = codex("pick");
    c.s.spawner.push(FakeRun::ok(&[DONE]));
    let row = c.b.send_with(&c.id, "hi", Some(json!({"model": "gpt-6"})));
    c.b.postmen.postman(&row.session).pass(c.b.at(T0));
    c.s.idle();
    let args = &c.s.spawner.specs()[0].args;
    assert!(args.windows(2).any(|w| w == ["-m", "gpt-6"]));
}

#[test]
fn codex_send_existing_session_queues_row() {
    let c = codex("queues");
    let first =
        c.s.call(
            ops::CODEX_SEND,
            json!({"id": c.id, "text": "one", "model": "gpt-6"}),
        )
        .unwrap();
    assert_eq!(first["id"], json!(c.id));
    assert_eq!(first["session"], CODEX_SESSION);
    assert_eq!(first["state"], "queued");
    assert_eq!(first["waiting"], 1);
    let outbox_id = first["outboxId"].as_str().unwrap().to_string();
    let second =
        c.s.call(ops::CODEX_SEND, json!({"id": c.id, "text": "two"}))
            .unwrap();
    assert_eq!(second["waiting"], 2);
    assert!(c.s.spawner.specs().is_empty(), "nothing started by the op");
    let row = c.b.row(&outbox_id).unwrap();
    assert_eq!(row.session, c.id);
    assert_eq!(row.text, "one");
    assert_eq!(row.options, Some(json!({"model": "gpt-6"})));
}

#[test]
fn codex_send_unknown_session_is_refused() {
    let c = codex("unknown");
    let (kind, message) = code(c.s.call(
        ops::CODEX_SEND,
        json!({"id": "codex:019a5c3e-ffff-7bbb-8ccc-00000000ffff", "text": "hi"}),
    ));
    assert_eq!(kind, ErrorCode::BadRequest);
    assert_eq!(message, "No such session.");
    let (kind, message) = code(c.b.outbox.call(
        &app(),
        ops::SESSION_SEND,
        &json!({"id": "codex:019a5c3e-ffff-7bbb-8ccc-00000000ffff", "text": "hi"}),
    ));
    assert_eq!(
        (kind, message.as_str()),
        (ErrorCode::BadRequest, "No such session.")
    );
    assert!(c.b.outbox.store().sessions().unwrap().is_empty());
}

#[test]
fn codex_send_draft_keeps_direct_path() {
    let c = codex("draft");
    c.s.spawner.push(FakeRun::ok(&[
        r#"{"type":"thread.started","thread_id":"019a5c3e-new0-7bbb-8ccc-000000000002"}"#,
        DONE,
    ]));
    let reply =
        c.s.call(
            ops::CODEX_SEND,
            json!({"draft_folder": c.s.work(), "text": "hi"}),
        )
        .unwrap();
    assert_eq!(reply["state"], "started");
    assert!(c.b.outbox.store().sessions().unwrap().is_empty());
    c.s.idle();
}

#[test]
fn session_send_to_a_codex_session_checks_it_exists() {
    let c = codex("exists");
    let sent =
        c.b.outbox
            .call(
                &app(),
                ops::SESSION_SEND,
                &json!({"id": c.id, "text": "hi"}),
            )
            .unwrap();
    assert_eq!(sent["state"], "queued");
}

struct AgyBench {
    rig: agy_t::Rig,
    b: Bench,
    courier: AgyCourier,
}

fn agy() -> AgyBench {
    let rig = agy_t::rig(true);
    let b = bench(
        Arc::new(Acting::in_memory(true)),
        rig.room.clone(),
        None,
        Some(&rig.ops),
    );
    let courier = AgyCourier::new(rig.ops.clone());
    AgyBench { rig, b, courier }
}

#[test]
fn runner_started_is_sent_without_transcript_agy() {
    let a = agy();
    let process = a.rig.loaded();
    let row = a.b.send(ROW, "Change hello.txt");
    assert_eq!(row.session, ROW);
    a.b.postmen.postman(&row.session).pass(a.b.at(T0));
    assert_eq!(a.b.row(&row.id), None);
    assert_eq!(a.b.removed()[0]["why"], "sent");
    assert_eq!(a.b.verifier.asked.load(Ordering::SeqCst), 0);
    assert_eq!(process.requests("session/prompt").len(), 1);
    assert!(!a.courier.verifies());
}

#[test]
fn busy_agy_waits_turn_then_prompts() {
    let a = agy();
    let process = a.rig.loaded();
    let first = a.b.send(ROW, "Change hello.txt");
    let mut p = a.b.postmen.postman(&first.session);
    p.pass(a.b.at(T0));
    a.rig.wait(crate::agy_ops::PERMISSION_PENDING);
    let second = a.b.send(ROW, "and then this");
    assert_eq!(p.pass(a.b.at(T0 + 10)), Next::At(T0 + 1_010));
    assert_eq!(
        a.b.state(&second.id),
        Some((State::Waiting, Some(Why::Turn)))
    );
    assert_eq!(process.requests("session/prompt").len(), 1);
    a.rig
        .call(
            PERMISSION,
            json!({"id": ROW, "rpc_id": "1", "option_id": "allow"}),
        )
        .unwrap();
    a.rig.change("turn_ended");
    p.pass(a.b.at(T0 + 1_010));
    assert_eq!(a.b.row(&second.id), None);
    assert_eq!(process.requests("session/prompt").len(), 2);
}

#[test]
fn agy_race_to_busy_waits_turn() {
    let a = agy();
    a.rig.loaded();
    let first = a.b.send(ROW, "Change hello.txt");
    a.b.postmen.postman(&first.session).pass(a.b.at(T0));
    a.rig.wait(crate::agy_ops::PERMISSION_PENDING);
    let row = a.b.send(ROW, "again");
    let taken = a.b.outbox.store().take(&row.id, T0).unwrap().unwrap();
    let (end, tried, note) = a.courier.deliver(&taken, false);
    assert_eq!(end, TryEnd::NotIdle);
    assert!(!tried.pasted && !tried.entered);
    assert_eq!(note, None);
    a.rig.call(CANCEL, json!({"id": ROW})).unwrap();
}

#[test]
fn agy_elsewhere_refused_at_send() {
    let a = agy();
    a.rig.ops.elsewhere().set_finder(
        Arc::new(|_| {
            Some(notchling_engine::tools::agy::elsewhere::Holder {
                pid: 4242,
                delegation: false,
            })
        }),
        std::time::Duration::from_secs(60),
    );
    let (kind, message) = code(a.b.outbox.call(
        &app(),
        ops::SESSION_SEND,
        &json!({"id": ROW, "text": "hi"}),
    ));
    assert_eq!(kind, ErrorCode::Failed);
    assert!(!message.is_empty());
    let (kind, _) = code(a.rig.call(ops::AGY_SEND, json!({"id": ROW, "text": "hi"})));
    assert_eq!(kind, ErrorCode::Failed);
    assert!(a.b.outbox.store().sessions().unwrap().is_empty());
}

#[test]
fn agy_held_later_waits_as_turn() {
    let a = agy();
    let row = a.b.send(ROW, "hi");
    let taken = a.b.outbox.store().take(&row.id, T0).unwrap().unwrap();
    a.rig.ops.elsewhere().set_finder(
        Arc::new(|_| {
            Some(notchling_engine::tools::agy::elsewhere::Holder {
                pid: 4242,
                delegation: true,
            })
        }),
        std::time::Duration::from_secs(60),
    );
    let (end, _, _) = a.courier.deliver(&taken, false);
    assert_eq!(end, TryEnd::NotIdle);
    assert!(a.rig.spawner.calls().is_empty());
}

#[test]
fn agy_send_unknown_session_is_refused() {
    let a = agy();
    let (kind, message) = code(a.b.outbox.call(
        &app(),
        ops::SESSION_SEND,
        &json!({"id": "agy:no-such-session", "text": "hi"}),
    ));
    assert_eq!(
        (kind, message.as_str()),
        (ErrorCode::BadRequest, "No such session.")
    );
}

#[test]
fn agy_send_existing_session_queues_row() {
    let a = agy();
    let reply = a
        .rig
        .call(ops::AGY_SEND, json!({"id": ROW, "text": "hi"}))
        .unwrap();
    assert_eq!(reply["id"], ROW);
    assert_eq!(reply["sent"], true);
    let outbox_id = reply["outboxId"].as_str().unwrap();
    let row = a.b.row(outbox_id).unwrap();
    assert_eq!((row.session.as_str(), row.text.as_str()), (ROW, "hi"));
    assert!(
        a.rig.spawner.calls().is_empty(),
        "nothing started by the op"
    );
}
