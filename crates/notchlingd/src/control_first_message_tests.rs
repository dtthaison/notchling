use crate::acting_ops_tests::{app, caller, sessions, setup_in, Setup, PROMPT};
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use crate::outbox_ops::OutboxOps;
use notchling_engine::control::{FakeLauncher, LaunchOutput, Launcher};
use notchling_engine::outbox::OutboxStore;
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, ApiError, Caller, ClientKind};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const JOB: &str = "6ce4c19c";
const TASK: &str = "Review the inbox design, then stop.";

const STATE: &str = r#"{"state":"idle","tempo":"idle","detail":"x","template":"bg","name":"main",
"inFlight":{"tasks":0,"queued":0,"kinds":[],"drainableMonitors":0},
"sessionId":"be50d36a-4348-4ba6-ab58-96959399a893","linkScanPath":"/nonexistent/x.jsonl",
"cwd":"/w/api","updatedAt":"2026-11-14T22:54:49.559Z"}"#;

struct Rig {
    _dir: TempDir,
    s: Setup,
    outbox: Arc<OutboxOps>,
    fake: Arc<FakeLauncher>,
}

fn rig(tag: &str, job_file: bool) -> Rig {
    let dir = TempDir::new(tag);
    if job_file {
        dir.write(&format!("claude/jobs/{JOB}/state.json"), STATE);
    }
    let roots = Roots {
        main_dir: dir.path("claude"),
        lanes_file: dir.path("none/lanes.json"),
    };
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok(&format!(
        "backgrounded · {JOB} · api (idle)\n"
    ))));
    let launcher: Arc<dyn Launcher> = fake.clone();
    let s = setup_in(true, PROMPT, sessions(), roots.clone(), Some(launcher));
    let outbox = Arc::new(OutboxOps::new(
        Arc::new(OutboxStore::in_memory_for_tests()),
        Arc::new(Mutex::new(json!([]))),
        roots,
        s.ops.acting_switch(),
        s.room.clone(),
        None,
    ));
    s.ops.attach_outbox(outbox.clone());
    Rig {
        _dir: dir,
        s,
        outbox,
        fake,
    }
}

fn node() -> Caller {
    caller(ClientKind::Node)
}

fn start(r: &Rig, who: &Caller, extra: Value) -> Result<Value, ApiError> {
    let mut args = json!({"folder": "/w/api"});
    for (key, value) in extra.as_object().unwrap() {
        args[key] = value.clone();
    }
    r.s.ops.call(who, ops::SESSION_NEW, &args)
}

fn queued(r: &Rig) -> Vec<Value> {
    r.outbox
        .call(&app(), ops::OUTBOX_LIST, &json!({}))
        .unwrap()
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[test]
fn a_session_a_node_starts_with_a_task_gets_it_as_its_first_message() {
    let r = rig("first", true);
    let reply = start(&r, &node(), json!({"text": TASK})).unwrap();
    assert_eq!(reply["jobId"], JOB);
    assert_eq!(reply["messageQueued"], true, "{reply}");
    assert_eq!(reply.get("warning"), None, "{reply}");
    let rows = queued(&r);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["text"], TASK);
    assert_eq!(rows[0]["session"], JOB);
    assert!(!format!("{:?}", r.fake.commands()).contains(TASK));
}

#[test]
fn a_task_that_cannot_be_queued_is_reported_and_the_session_still_starts() {
    let r = rig("lost", false);
    let reply = start(&r, &node(), json!({"text": TASK})).unwrap();
    assert_eq!(reply["jobId"], JOB);
    assert_eq!(reply["messageQueued"], false, "{reply}");
    assert!(
        reply["warning"].as_str().unwrap().contains("first message"),
        "{reply}"
    );
    assert!(queued(&r).is_empty());
}

#[test]
fn only_a_node_start_takes_the_text_and_a_start_without_it_queues_nothing() {
    let r = rig("plain", true);
    let reply = start(&r, &node(), json!({})).unwrap();
    assert_eq!(reply, json!({"jobId": JOB}));
    let r = rig("app", true);
    let reply = start(&r, &app(), json!({"text": TASK})).unwrap();
    assert_eq!(reply, json!({"jobId": JOB}));
    assert!(queued(&r).is_empty());
}
