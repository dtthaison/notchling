use super::{Parts, Served, SetView, Wire, SEND_WAIT};
use crate::limits_ops_tests::TempDir;
use notchling_engine::bridge::model::{
    Control, PendingPrompt, PromptKind, PromptQuestion, Session, SessionState,
};
use notchling_engine::bridge::relay::{Call, Cancelled, HeldAct, Relay, Routed};
use notchling_engine::bridge::BridgeFailure;
use notchling_engine::cluster::persist;
use notchling_engine::cluster::wire::WireError;
use notchling_engine::cluster::NodeId;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

const M5: &str = "0123456789abcdef";
const M1: &str = "fedcba9876543210";
const W: &str = "1111222233334444";
const M2: &str = "aaaabbbbccccdddd";
const T0: i64 = 1_800_000_000_000;
const HOUR: i64 = 3_600_000;

struct Net {
    status: Mutex<Value>,
    sent: Mutex<Vec<(String, String, Value)>>,
    reply: Mutex<Result<Value, WireError>>,
}

impl Wire for Net {
    fn status(&self) -> Value {
        self.status.lock().unwrap().clone()
    }

    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        within: std::time::Duration,
    ) -> Result<Value, WireError> {
        assert_eq!(within, SEND_WAIT);
        self.sent
            .lock()
            .unwrap()
            .push((to.as_str().to_string(), op.to_string(), args));
        self.reply.lock().unwrap().clone()
    }
}

struct Set {
    newest: Mutex<Option<i64>>,
    kept: PathBuf,
}

impl SetView for Set {
    fn newest_ms(&self) -> Option<i64> {
        *self.newest.lock().unwrap()
    }

    fn transcript(&self, original: &Path) -> Option<PathBuf> {
        (original == Path::new("/Users/example/.claude/projects/app/job-7.jsonl"))
            .then(|| self.kept.clone())
    }
}

fn member(id: &str, name: &str, role: &str, can: bool, online: bool) -> Value {
    json!({
        "id": id, "name": name, "role": role, "canBeMaster": can, "online": online,
        "client": true, "headless": false, "reference": false, "state": "active",
        "addrs": [format!("10.0.0.{}", name.len())], "fingerprint": format!("{id}{id}{id}{id}"),
    })
}

fn status(me: &str, home: &str, online: bool) -> Value {
    let role = |id: &str, standby: &'static str| -> &'static str {
        if id == home {
            "home"
        } else {
            standby
        }
    };
    json!({
        "cluster": {"id": "c", "name": "c", "term": {"n": 3, "node": home}, "home": home},
        "me": me,
        "members": [
            member(M5, "m5", role(M5, "standby"), true, online || me == M5),
            member(M1, "m1", role(M1, "standby"), true, true),
            member(M2, "m2", role(M2, "standby"), true, true),
            member(W, "worker", "worker", false, true),
        ],
    })
}

struct Rig {
    _dir: TempDir,
    dir: PathBuf,
    net: Arc<Net>,
    set: Arc<Set>,
    clock: Arc<AtomicI64>,
    rows: Arc<Mutex<Vec<String>>>,
    relay: Arc<super::HeldRelay>,
}

struct Env<'a> {
    dir: &'a Path,
    net: &'a Arc<Net>,
    set: &'a Arc<Set>,
    clock: &'a Arc<AtomicI64>,
    rows: &'a Arc<Mutex<Vec<String>>>,
}

fn parts(env: Env<'_>) -> Parts {
    let clock = env.clock.clone();
    let rows = env.rows.clone();
    Parts {
        wire: env.net.clone(),
        dir: env.dir.to_path_buf(),
        clock: Arc::new(move || clock.load(Ordering::SeqCst)),
        prompts: Arc::new(|id| {
            (id == format!("{M5}/p1")).then(|| PendingPrompt {
                id: id.to_string(),
                session_id: format!("{M5}/job-7"),
                kind: PromptKind::Question(vec![PromptQuestion {
                    header: "Pick".into(),
                    question: "Which?".into(),
                    options: vec!["Red".into(), "Blue".into()],
                    multi_select: false,
                }]),
                tool_name: String::new(),
                summary: String::new(),
                created_ms: 0.0,
            })
        }),
        model: Arc::new(|short| (short == "opus").then(|| "claude-opus-5-5".to_string())),
        rows: Arc::new(move || rows.lock().unwrap().clone()),
        set: env.set.clone(),
        bridge: Arc::new(|| Served {
            port: 47821,
            fingerprint: Some("5".repeat(64)),
        }),
    }
}

fn rig(me: &str, home: &str, online: bool) -> Rig {
    let dir = TempDir::new("relay");
    let cluster = dir.path("cluster");
    std::fs::create_dir_all(&cluster).unwrap();
    let net = Arc::new(Net {
        status: Mutex::new(status(me, home, online)),
        sent: Mutex::new(Vec::new()),
        reply: Mutex::new(Ok(json!({"ok": true}))),
    });
    let set = Arc::new(Set {
        newest: Mutex::new(Some(T0 - 90_000)),
        kept: dir.path("kept.jsonl"),
    });
    let clock = Arc::new(AtomicI64::new(T0));
    let rows = Arc::new(Mutex::new(vec![
        format!("{M5}/job-7"),
        format!("{W}/job-3"),
    ]));
    let relay = Arc::new(super::HeldRelay::new(parts(Env {
        dir: &cluster,
        net: &net,
        set: &set,
        clock: &clock,
        rows: &rows,
    })));
    Rig {
        _dir: dir,
        dir: cluster,
        net,
        set,
        clock,
        rows,
        relay,
    }
}

fn reborn(rig: &Rig) -> super::HeldRelay {
    super::HeldRelay::new(parts(Env {
        dir: &rig.dir,
        net: &rig.net,
        set: &rig.set,
        clock: &rig.clock,
        rows: &rig.rows,
    }))
}

fn session(id: &str) -> Session {
    Session::new(
        id,
        "job",
        "/Users/example/src/app",
        Control::Job(id.to_string()),
        SessionState::Done,
    )
}

fn message(rig: &Rig, request: &str, session_id: &str, text: &str) -> Routed {
    let act = HeldAct::Message {
        session_id: session_id.to_string(),
        text: text.to_string(),
    };
    let found = session(session_id);
    rig.relay.route(&Call {
        device: "phone-1",
        request,
        act: &act,
        session: Some(&found),
    })
}

fn sent(rig: &Rig) -> Vec<(String, String, Value)> {
    rig.net.sent.lock().unwrap().clone()
}

fn go(rig: &Rig, me: &str, home: &str, online: bool) {
    *rig.net.status.lock().unwrap() = status(me, home, online);
}

fn unreachable_error() -> WireError {
    WireError::new("offline", "m5 is offline")
}

#[test]
fn hello_lists_can_be_master_machines() {
    let rig = rig(M1, M5, true);
    let cluster = rig.relay.cluster().expect("a cluster");
    assert_eq!(cluster["primary"], M5);
    assert_eq!(cluster["term"], 3);
    let nodes = cluster["nodes"].as_array().unwrap();
    let names: Vec<&str> = nodes.iter().map(|n| n["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["m5", "m1", "m2"], "workers are left out");
    let me = nodes.iter().find(|n| n["id"] == M1).unwrap();
    assert_eq!(me["fingerprint"], "5".repeat(64), "the served certificate");
    assert_eq!(me["addrs"], json!(["10.0.0.2:47821"]));
    assert_eq!(me["canBeMaster"], true);
    let other = nodes.iter().find(|n| n["id"] == M5).unwrap();
    assert_eq!(other["fingerprint"], format!("{M5}{M5}{M5}{M5}"));
    go(&rig, M1, M5, false);
    let cluster = rig.relay.cluster().unwrap();
    assert_eq!(cluster["nodes"][0]["online"], false);
}

#[test]
fn a_machine_in_no_cluster_says_nothing() {
    let rig = rig(M1, M5, true);
    *rig.net.status.lock().unwrap() = json!({"cluster": null, "members": [], "me": M1});
    assert_eq!(rig.relay.cluster(), None);
    assert_eq!(
        message(&rig, "1", &format!("{M5}/job-7"), "hi"),
        Routed::Local
    );
}

#[test]
fn standby_relays_to_reachable_primary() {
    let rig = rig(M1, M5, true);
    let routed = message(&rig, "1", &format!("{M5}/job-7"), "go on");
    assert_eq!(routed, Routed::Done(json!({"sent": true, "relayed": true})));
    assert_eq!(
        sent(&rig),
        [(
            M5.to_string(),
            "session.reply".to_string(),
            json!({"id": "job-7", "text": "go on"})
        )]
    );
}

#[test]
fn worker_session_goes_straight_to_worker() {
    let rig = rig(M1, M5, false);
    assert_eq!(
        message(&rig, "1", &format!("{W}/job-3"), "hi"),
        Routed::Local
    );
    assert!(sent(&rig).is_empty());
    assert!(rig.relay.list().entries.is_empty());
}

#[test]
fn the_primary_and_a_worker_never_hold() {
    let rig = rig(M5, M5, true);
    assert_eq!(message(&rig, "1", "job-7", "hi"), Routed::Local);
    let rig = self::rig(W, M5, false);
    assert_eq!(
        message(&rig, "1", &format!("{M5}/job-7"), "hi"),
        Routed::Local
    );
}

#[test]
fn unreachable_primary_holds_in_order() {
    let rig = rig(M1, M5, false);
    let first = message(&rig, "1", &format!("{M5}/job-7"), "one");
    let second = message(&rig, "2", &format!("{M5}/job-7"), "two");
    let (Routed::Held(a), Routed::Held(b)) = (first, second) else {
        panic!("both are held");
    };
    assert_ne!(a.id, b.id);
    assert_eq!(a.at_ms, T0);
    let list = rig.relay.list();
    let summaries: Vec<&str> = list.entries.iter().map(|e| e.summary.as_str()).collect();
    assert_eq!(summaries, ["one", "two"]);
    assert_eq!(list.entries[0].op, "session.message");
    assert_eq!(
        list.entries[0].session_id.as_deref(),
        Some(&*format!("{M5}/job-7"))
    );
    assert!(!list.entries[0].waiting);
    assert!(sent(&rig).is_empty());
    let again = message(&rig, "1", &format!("{M5}/job-7"), "one");
    assert_eq!(again, Routed::Held(a), "the same request is the same entry");
    assert_eq!(rig.relay.list().entries.len(), 2);
}

#[test]
fn a_refused_send_while_reachable_holds_when_the_primary_drops() {
    let rig = rig(M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(unreachable_error());
    assert!(matches!(
        message(&rig, "1", &format!("{M5}/job-7"), "one"),
        Routed::Held(_)
    ));
    assert_eq!(rig.relay.list().entries.len(), 1);
}

#[test]
fn a_refusal_is_an_error_and_a_timeout_is_not_resent() {
    let rig = rig(M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(WireError::new("bad_args", "no such session"));
    let Routed::Fail(failure) = message(&rig, "1", &format!("{M5}/job-7"), "x") else {
        panic!("a failure");
    };
    assert!(failure.message.contains("no such session"));
    *rig.net.reply.lock().unwrap() = Err(WireError::new("timeout", "m5 didn't answer"));
    let routed = message(&rig, "2", &format!("{M5}/job-7"), "y");
    assert_eq!(
        routed,
        Routed::Fail(BridgeFailure::primary_unreachable(
            "m5",
            "m5 didn't answer, so that may or may not have arrived."
        ))
    );
    assert!(rig.relay.list().entries.is_empty(), "never held twice");
}

#[test]
fn held_command_is_delivered_once_on_return() {
    let rig = rig(M1, M5, false);
    let Routed::Held(one) = message(&rig, "1", &format!("{M5}/job-7"), "one") else {
        panic!("held");
    };
    message(&rig, "2", &format!("{M5}/job-7"), "two");
    rig.relay.flush();
    assert!(sent(&rig).is_empty(), "still offline");
    go(&rig, M1, M5, true);
    rig.relay.flush();
    rig.relay.flush();
    let texts: Vec<Value> = sent(&rig).iter().map(|s| s.2["text"].clone()).collect();
    assert_eq!(texts, [json!("one"), json!("two")], "once, in order");
    assert!(rig.relay.list().entries.is_empty());
    assert_eq!(rig.relay.cancel(&one.id), Cancelled::Delivered);
    assert_eq!(rig.relay.cancel("nope"), Cancelled::Unknown);
}

#[test]
fn a_held_command_that_finds_the_primary_gone_again_stays_held() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "one");
    message(&rig, "2", &format!("{M5}/job-7"), "two");
    go(&rig, M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(unreachable_error());
    rig.relay.flush();
    assert_eq!(sent(&rig).len(), 1, "stops at the first that fails");
    let summaries: Vec<String> = rig
        .relay
        .list()
        .entries
        .iter()
        .map(|e| e.summary.clone())
        .collect();
    assert_eq!(summaries, ["one", "two"]);
    *rig.net.reply.lock().unwrap() = Ok(json!({}));
    rig.relay.flush();
    assert!(rig.relay.list().entries.is_empty());
    assert_eq!(sent(&rig).len(), 3);
}

#[test]
fn held_goes_before_new_ops() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "held one");
    go(&rig, M1, M5, true);
    let routed = message(&rig, "2", &format!("{M5}/job-7"), "new one");
    assert!(matches!(routed, Routed::Done(_)));
    let texts: Vec<Value> = sent(&rig).iter().map(|s| s.2["text"].clone()).collect();
    assert_eq!(texts, [json!("held one"), json!("new one")]);
}

#[test]
fn a_new_op_waits_behind_a_held_one_that_cannot_go() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "held one");
    go(&rig, M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(unreachable_error());
    let routed = message(&rig, "2", &format!("{M5}/job-7"), "new one");
    assert!(matches!(routed, Routed::Held(_)));
    assert_eq!(rig.relay.list().entries.len(), 2);
}

#[test]
fn held_runs_here_after_promote() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "keep going");
    go(&rig, M1, M1, true);
    *rig.rows.lock().unwrap() = vec![format!("{M1}/job-7")];
    rig.relay.flush();
    assert_eq!(
        sent(&rig),
        [(
            M1.to_string(),
            "session.reply".to_string(),
            json!({"id": "job-7", "text": "keep going"})
        )],
        "it runs on this machine now"
    );
    assert!(rig.relay.list().entries.is_empty());
}

#[test]
fn a_command_for_a_session_that_didnt_move_stays_held_after_a_flush() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "keep going");
    go(&rig, M1, M2, true);
    *rig.rows.lock().unwrap() = vec![format!("{M5}/job-7")];
    rig.relay.flush();
    assert!(sent(&rig).is_empty());
    assert_eq!(rig.relay.list().entries.len(), 1);
    *rig.rows.lock().unwrap() = vec![format!("{M2}/job-7")];
    rig.relay.flush();
    assert_eq!(sent(&rig)[0].0, M2);
    assert!(rig.relay.list().entries.is_empty());
}

#[test]
fn held_answer_to_a_closed_prompt_is_dropped_and_reported() {
    let rig = rig(M1, M5, false);
    let act = HeldAct::Answer {
        request_id: format!("{M5}/p1"),
        session_id: format!("{M5}/job-7"),
        allow: None,
        answers: Some(vec!["Blue".into()]),
    };
    let found = session(&format!("{M5}/job-7"));
    let routed = rig.relay.route(&Call {
        device: "phone-1",
        request: "9",
        act: &act,
        session: Some(&found),
    });
    assert!(matches!(routed, Routed::Held(_)));
    go(&rig, M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(WireError::new("failed", "no such prompt"));
    rig.relay.flush();
    rig.relay.flush();
    assert_eq!(
        sent(&rig),
        [(
            M5.to_string(),
            "session.answer".to_string(),
            json!({"id": "p1", "answers": [[2]]})
        )],
        "tried once, as an option number"
    );
    let list = rig.relay.list();
    assert!(list.entries.is_empty());
    assert_eq!(list.notices.len(), 1);
    assert_eq!(
        list.notices[0].reason,
        "m5 closed that question before your answer arrived"
    );
}

#[test]
fn a_held_allow_goes_as_a_permission_decision() {
    let rig = rig(M1, M5, false);
    let act = HeldAct::Answer {
        request_id: format!("{M5}/p2"),
        session_id: format!("{M5}/job-7"),
        allow: Some(true),
        answers: None,
    };
    let found = session(&format!("{M5}/job-7"));
    rig.relay.route(&Call {
        device: "phone-1",
        request: "9",
        act: &act,
        session: Some(&found),
    });
    go(&rig, M1, M5, true);
    rig.relay.flush();
    assert_eq!(sent(&rig)[0].1, "session.permission");
}

#[test]
fn reads_are_answered_from_the_set_marked_stale() {
    let rig = rig(M1, M5, false);
    assert_eq!(rig.relay.stale_ms(), Some(T0 - 90_000));
    let mut found = session(&format!("{M5}/job-7"));
    found.transcript_path = Some("/Users/example/.claude/projects/app/job-7.jsonl".into());
    assert_eq!(rig.relay.set_transcript(&found), Some(rig.set.kept.clone()));
    let mut worker = session(&format!("{W}/job-3"));
    worker.transcript_path = found.transcript_path.clone();
    assert_eq!(rig.relay.set_transcript(&worker), None);
    go(&rig, M1, M5, true);
    assert_eq!(rig.relay.stale_ms(), None);
    assert_eq!(rig.relay.set_transcript(&found), None);
}

#[test]
fn phone_session_new_relays_to_primary() {
    let rig = rig(M1, M5, true);
    let act = HeldAct::NewSession {
        cwd: "/Users/example/src/app".into(),
        model: Some("opus".into()),
        model_id: None,
    };
    let routed = rig.relay.route(&Call {
        device: "phone-1",
        request: "17",
        act: &act,
        session: None,
    });
    assert_eq!(
        routed,
        Routed::Done(json!({"started": true, "relayed": true}))
    );
    assert_eq!(
        sent(&rig),
        [(
            M5.to_string(),
            "session.new".to_string(),
            json!({"folder": "/Users/example/src/app", "flags": ["--model", "claude-opus-5-5"]})
        )]
    );
}

#[test]
fn an_old_entry_waits_for_the_phone_to_confirm_it() {
    let rig = rig(M1, M5, false);
    let Routed::Held(old) = message(&rig, "1", &format!("{M5}/job-7"), "old") else {
        panic!("held");
    };
    rig.clock.store(T0 + HOUR + 1, Ordering::SeqCst);
    go(&rig, M1, M5, true);
    rig.relay.flush();
    assert!(sent(&rig).is_empty(), "older than an hour waits");
    let list = rig.relay.list();
    assert!(list.entries[0].waiting);
    assert!(!rig.relay.confirm("nope"));
    assert!(rig.relay.confirm(&old.id));
    assert_eq!(sent(&rig).len(), 1, "confirmed, it goes now");
    assert!(rig.relay.list().entries.is_empty());
}

#[test]
fn a_held_entry_survives_a_handover_on_the_holding_machine() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "still here");
    let db = rig.dir.join(persist::CLUSTER_DB);
    assert!(db.exists());
    for name in ["cluster.db", "cluster.db-wal", "cluster.db-shm"] {
        let _ = std::fs::remove_file(rig.dir.join(name));
    }
    assert!(persist::load_held(&rig.dir).unwrap().list().is_empty());
    rig.relay.carry();
    let kept = persist::load_held(&rig.dir).unwrap().list();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].op, "session.message");
    let reborn = reborn(&rig);
    assert_eq!(reborn.list().entries.len(), 1, "a restart finds it too");
}
