use super::{pending, PhoneCluster, FOLLOW_EVERY_MS, FOLLOW_RETRY_MS};
use crate::acting_ops_tests::{setup, FakeRoom};
use crate::bridge_backend::DaemonActing;
use crate::cluster_forward::route;
use crate::cluster_prompts::{Job, PromptSource, RemotePrompts};
use crate::cluster_sessions::{agent_row, Peer, RemoteSessions, RowSource};
use crate::cluster_switches::LateHandler;
use crate::every_session_tests::sources;
use crate::limits_ops_tests::TempDir;
use notchling_engine::bridge::live::{Acting, Sources};
use notchling_engine::bridge::model::{
    Control, PendingPrompt, PromptKind, PromptResponse, Session,
};
use notchling_engine::cluster::NodeId;
use notchling_local_api::{ApiError, Caller, ClientKind, ErrorCode, Handler, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn node() -> NodeId {
    NodeId::parse("0123456789abcdef").unwrap()
}

fn peer() -> Peer {
    Peer {
        id: node(),
        name: "mac-mini".into(),
        online: true,
    }
}

const ROW: &str = "0123456789abcdef/job-7";
const PROMPT: &str = "0123456789abcdef/p1";

struct Rows(Value);

impl RowSource for Rows {
    fn rows(&self, _: &NodeId) -> Result<Value, ApiError> {
        Ok(self.0.clone())
    }
}

struct Follows;

impl PromptSource for Follows {
    fn follow(&self, _: &NodeId) -> Result<(), ApiError> {
        Ok(())
    }
    fn list(&self, _: &NodeId) -> Result<Value, ApiError> {
        Ok(json!([]))
    }
}

type Call = (Tier, ClientKind, String, Value);

#[derive(Default)]
struct Seen {
    calls: Mutex<Vec<Call>>,
    refuse_follow: AtomicBool,
}

impl Seen {
    fn ops(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.2.clone())
            .collect()
    }
}

impl Handler for Seen {
    fn spec(&self, _: &str) -> Option<OpSpec> {
        None
    }
    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        self.calls
            .lock()
            .unwrap()
            .push((caller.tier, caller.client, op.to_string(), args.clone()));
        if op == "cluster.follow" && self.refuse_follow.load(Ordering::SeqCst) {
            return Err(ApiError::new(ErrorCode::Failed, "no such session"));
        }
        Ok(json!({}))
    }
}

struct World {
    cluster: Arc<PhoneCluster>,
    seen: Arc<Seen>,
    dir: TempDir,
    _handler: Arc<dyn Handler>,
}

fn world() -> World {
    let dir = TempDir::new("bridge-cluster");
    let room = Arc::new(FakeRoom::default());
    let sessions = Arc::new(RemoteSessions::new(dir.path("mirror")));
    sessions.sync(&[peer()], &Rows(json!([agent_row("job-7")])), 0);
    let inline: crate::cluster_prompts::Spawn = Arc::new(|job: Job| {
        job();
        Ok(())
    });
    let prompts = RemotePrompts::with_spawn(room, sessions.clone(), inline);
    prompts.sync(&[peer()], true, &Arc::new(Follows), 0);
    prompts.on_event(
        &node(),
        "prompt.pending",
        &json!({
            "id": "p1", "sessionId": "job-7", "kind": "permission", "toolName": "Bash",
            "summary": "make", "input": {}, "canAlwaysAllow": true, "question": null,
            "cwd": "/w", "transcriptPath": null, "createdMs": 7,
        }),
    );
    let seen = Arc::new(Seen::default());
    let handler: Arc<dyn Handler> = seen.clone();
    let late = Arc::new(LateHandler::default());
    late.serve(&handler);
    let cluster = PhoneCluster::none();
    cluster.attach(late, sessions, prompts);
    World {
        cluster,
        seen,
        dir,
        _handler: handler,
    }
}

fn remote_session(w: &World) -> Session {
    let mut source = sources(
        &w.dir.path("."),
        Arc::new(Mutex::new(json!([]))),
        Arc::new(crate::every_session::EverySession::none()),
    );
    source.cluster = w.cluster.clone();
    source
        .claude_sessions()
        .into_iter()
        .find(|s| s.id == ROW)
        .expect("the remote session")
}

fn acting(cluster: &Arc<PhoneCluster>) -> DaemonActing {
    DaemonActing {
        ops: Arc::new(setup(true, &[], json!([])).ops),
        sessions: Arc::new(Mutex::new(json!([]))),
        cluster: cluster.clone(),
    }
}

#[test]
fn the_phones_list_has_remote_rows_with_their_node_and_prompt() {
    let w = world();
    let session = remote_session(&w);
    let node = session.node.clone().expect("node");
    assert_eq!(
        (node.id.as_str(), node.name.as_str(), node.online),
        ("0123456789abcdef", "mac-mini", true)
    );
    assert_eq!(session.control, Control::Job(ROW.into()));
    let prompt = session.pending.expect("the remote prompt");
    assert_eq!(
        (prompt.id.as_str(), prompt.session_id.as_str()),
        (PROMPT, ROW)
    );
    assert_eq!(prompt.kind, PromptKind::Permission);

    let none = PhoneCluster::none();
    let mut source = sources(
        &w.dir.path("."),
        Arc::new(Mutex::new(json!([]))),
        Arc::new(crate::every_session::EverySession::none()),
    );
    source.cluster = none;
    assert!(source.claude_sessions().is_empty());
}

#[test]
fn phone_reply_to_remote_session_is_forwarded() {
    let w = world();
    let session = remote_session(&w);
    let acting = acting(&w.cluster);
    acting.send_message("keep going", &session).unwrap();
    acting.send_now(&session).unwrap();
    let calls = w.seen.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![
            (
                Tier::App,
                ClientKind::App,
                "session.reply".to_string(),
                json!({"id": ROW, "text": "keep going"})
            ),
            (
                Tier::App,
                ClientKind::App,
                "session.interrupt".to_string(),
                json!({"id": ROW})
            ),
        ]
    );
}

#[test]
fn phone_answers_a_remote_prompt_with_allow_or_deny_only() {
    let w = world();
    let acting = acting(&w.cluster);
    let prompt = remote_session(&w).pending.unwrap();
    acting.respond(&prompt, &PromptResponse::Allow).unwrap();
    acting.respond(&prompt, &PromptResponse::Deny).unwrap();
    let calls = w.seen.calls.lock().unwrap().clone();
    let sent: Vec<(String, Value)> = calls.iter().map(|c| (c.2.clone(), c.3.clone())).collect();
    assert_eq!(
        sent,
        vec![
            (
                "session.permission".to_string(),
                json!({"id": PROMPT, "decision": "allow"})
            ),
            (
                "session.permission".to_string(),
                json!({"id": PROMPT, "decision": "deny"})
            ),
        ]
    );
    assert!(!format!("{sent:?}").to_lowercase().contains("always"));
    assert!(calls.iter().all(|c| c.0 == Tier::App));
}

#[test]
fn remote_prompts_become_phone_prompts() {
    let question = json!({
        "id": "0123456789abcdef/q1", "sessionId": "s", "kind": "question",
        "question": {"items": []}, "createdMs": 1,
    });
    assert!(pending(&question).is_none());
    let odd = json!({"id": "0123456789abcdef/q2", "kind": "unknown"});
    assert_eq!(
        pending(&odd).map(|p| p.kind),
        Some(PromptKind::Permission),
        "anything that isn't a question is a permission, as for local prompts"
    );
    let real: PendingPrompt = pending(&json!({
        "id": "0123456789abcdef/q3", "sessionId": "job-7", "rowId": ROW, "kind": "question",
        "question": {"items": [{"header": "DB", "question": "Which?", "options": ["A", "B"], "multiSelect": true}]},
        "createdMs": 2,
    }))
    .unwrap();
    assert_eq!(real.session_id, ROW);
    match real.kind {
        PromptKind::Question(items) => {
            assert_eq!(items[0].options, ["A", "B"]);
            assert!(items[0].multi_select);
        }
        PromptKind::Permission => panic!("a question"),
    }
}

#[test]
fn follow_is_sent_once_a_minute_while_the_session_is_open() {
    let w = world();
    w.cluster.follow(ROW, 0);
    w.cluster.follow(ROW, FOLLOW_EVERY_MS - 1);
    assert_eq!(w.seen.ops(), ["cluster.follow"]);
    w.cluster.follow(ROW, FOLLOW_EVERY_MS);
    assert_eq!(w.seen.ops().len(), 2);
    let first = w.seen.calls.lock().unwrap()[0].clone();
    assert_eq!((first.0, first.3), (Tier::App, json!({"id": ROW})));
}

#[test]
fn a_refused_follow_is_retried_soon_but_not_on_every_poll() {
    let w = world();
    w.seen.refuse_follow.store(true, Ordering::SeqCst);
    w.cluster.follow(ROW, 0);
    w.cluster.follow(ROW, FOLLOW_RETRY_MS - 1);
    assert_eq!(w.seen.ops().len(), 1);
    w.cluster.follow(ROW, FOLLOW_RETRY_MS);
    assert_eq!(w.seen.ops().len(), 2);
}

#[test]
fn without_the_cluster_a_remote_reply_is_refused_plainly() {
    let w = world();
    let session = remote_session(&w);
    let refused = acting(&PhoneCluster::none())
        .send_message("hi", &session)
        .unwrap_err();
    assert_eq!(
        refused.message,
        "mac-mini is not reachable from the phone yet."
    );
}

#[test]
fn a_remote_view_only_row_cannot_be_replied_to() {
    let mut session = Session::new(
        ROW,
        "x",
        "/w",
        Control::ViewOnly,
        notchling_engine::bridge::model::SessionState::Done,
    );
    session.node = Some(notchling_engine::bridge::model::SessionNode {
        id: "0123456789abcdef".into(),
        name: "mac-mini".into(),
        online: true,
    });
    let w = world();
    let refused = acting(&w.cluster).send_message("hi", &session).unwrap_err();
    assert_eq!(refused.message, "This session is view only.");
    assert!(w.seen.ops().is_empty());
}

#[test]
fn every_call_the_phone_makes_on_a_remote_id_has_a_route_to_its_owner() {
    let me = NodeId::parse("fedcba9876543210").unwrap();
    for (op, args) in [
        ("session.reply", json!({"id": ROW, "text": "x"})),
        ("session.interrupt", json!({"id": ROW})),
        (
            "session.permission",
            json!({"id": PROMPT, "decision": "allow"}),
        ),
        ("session.answer", json!({"id": PROMPT, "answers": []})),
    ] {
        let (to, bare) = route(op, &args, &me).unwrap_or_else(|| panic!("{op} has no route"));
        assert_eq!(to, node());
        assert!(!bare["id"].as_str().unwrap().contains('/'), "{op}");
    }
    assert!(route("cluster.follow", &json!({"id": ROW}), &me).is_none());
}
