use super::{
    clean_pending, NodePush, PromptFollower, PromptSource, RemotePrompts, FOLLOW_EVERY_MS,
    MAX_PER_NODE, TEXT_MAX,
};
use crate::acting_ops::Room;
use crate::acting_ops_tests::{app, caller, sessions, setup, Setup, PROMPT};
use crate::cluster_forward::{Forward, NodeForward, PromptFollowers, PromptPush};
use crate::cluster_node::{Hooks, LocalCalls};
use crate::cluster_sessions::{
    agent_row, peers_of, HandlerRows, Peer, RemoteSessions, RowSource, SessionCalls,
};
use crate::cluster_switches::LateHandler;
use crate::cluster_testkit::{eventually, pair, Dir, Opts, TestNode};
use crate::cluster_wiring::DaemonCalls;
use crate::daemon::Ops;
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::{qualify, NodeId};
use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode, Handler, Tier};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

fn node(n: u8) -> NodeId {
    NodeId::parse(&format!("{n:016x}")).unwrap()
}

fn peer(n: u8, online: bool) -> Peer {
    Peer {
        id: node(n),
        name: format!("n{n}"),
        online,
    }
}

fn prompt(id: &str) -> Value {
    json!({
        "id": id, "sessionId": "s1", "kind": "permission", "toolName": "Bash",
        "summary": "ls", "input": {"command": "ls"}, "canAlwaysAllow": true,
        "question": null, "cwd": "/w", "transcriptPath": "/w/s1.jsonl", "createdMs": 5,
    })
}

#[derive(Default)]
struct FakeSource {
    follows: Mutex<Vec<NodeId>>,
    lists: Mutex<Vec<NodeId>>,
    rows: Mutex<BTreeMap<NodeId, Value>>,
}

impl FakeSource {
    fn follows(&self) -> usize {
        self.follows.lock().unwrap().len()
    }

    fn set(&self, node: &NodeId, rows: Value) {
        self.rows.lock().unwrap().insert(node.clone(), rows);
    }
}

impl PromptSource for FakeSource {
    fn follow(&self, node: &NodeId) -> Result<(), ApiError> {
        self.follows.lock().unwrap().push(node.clone());
        Ok(())
    }

    fn list(&self, node: &NodeId) -> Result<Value, ApiError> {
        self.lists.lock().unwrap().push(node.clone());
        Ok(self
            .rows
            .lock()
            .unwrap()
            .get(node)
            .cloned()
            .unwrap_or_else(|| json!([])))
    }
}

struct OneRow(Value);

impl RowSource for OneRow {
    fn rows(&self, _node: &NodeId) -> Result<Value, ApiError> {
        Ok(self.0.clone())
    }
}

struct Unit {
    room: Arc<crate::acting_ops_tests::FakeRoom>,
    remote: Arc<RemoteSessions>,
    prompts: RemotePrompts,
    dir: Dir,
}

fn unit() -> Unit {
    let dir = Dir::new("prompts");
    let room = Arc::new(crate::acting_ops_tests::FakeRoom::default());
    let remote = Arc::new(RemoteSessions::new(dir.path().join("mirror")));
    let prompts = RemotePrompts::new(room.clone(), remote.clone());
    Unit {
        room,
        remote,
        prompts,
        dir,
    }
}

fn following(u: &Unit, source: &FakeSource, n: u8) {
    u.prompts.sync(&[peer(n, true)], true, source, 0);
}

#[test]
fn client_with_app_sends_node_follow_and_renews() {
    let u = unit();
    let source = FakeSource::default();
    let peers = [peer(1, true), peer(2, true), peer(3, false)];
    u.prompts.sync(&peers, true, &source, 0);
    assert_eq!(*source.follows.lock().unwrap(), vec![node(1), node(2)]);
    assert_eq!(source.lists.lock().unwrap().len(), 2, "filled on link up");
    u.prompts.sync(&peers, true, &source, FOLLOW_EVERY_MS - 1);
    assert_eq!(source.follows(), 2, "the lease is renewed only every 30 s");
    u.prompts.sync(&peers, true, &source, FOLLOW_EVERY_MS);
    assert_eq!(source.follows(), 4);
    assert_eq!(
        source.lists.lock().unwrap().len(),
        2,
        "listed once per link"
    );
}

#[test]
fn worker_never_sends_node_follow() {
    let net = MemNet::new();
    let mut home = Opts::named("home");
    home.client = true;
    let (a, b) = pair(&net, home, Opts::named("worker"));
    assert!(super::wants_follow(&a.node.status(), true));
    assert!(!super::wants_follow(&a.node.status(), false));
    assert!(!super::wants_follow(&b.node.status(), true));
}

#[test]
fn follow_stops_when_app_detaches() {
    let u = unit();
    let source = FakeSource::default();
    following(&u, &source, 1);
    u.prompts
        .on_event(&node(1), "prompt.pending", &prompt("p1"));
    assert_eq!(u.prompts.list().len(), 1);
    u.prompts.sync(&[peer(1, true)], false, &source, 1_000);
    assert!(u.prompts.list().is_empty());
    let resolved = u.room.named("prompt.resolved");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0]["id"], json!(format!("{}/p1", node(1))));
    u.prompts
        .sync(&[peer(1, true)], false, &source, FOLLOW_EVERY_MS * 3);
    assert_eq!(source.follows(), 1, "nothing is sent while detached");
    u.prompts
        .on_event(&node(1), "prompt.pending", &prompt("p2"));
    assert!(u.prompts.list().is_empty(), "a late push is ignored");
}

#[derive(Default)]
struct Pushed(Mutex<Vec<(Vec<String>, String)>>);

impl PromptPush for Pushed {
    fn push(&self, nodes: &[String], event: &str, _data: &Value) {
        self.0
            .lock()
            .unwrap()
            .push((nodes.to_vec(), event.to_string()));
    }
}

#[test]
fn owner_pushes_prompt_only_to_followers() {
    let followers = PromptFollowers::default();
    let pushed = Arc::new(Pushed::default());
    followers.attach(pushed.clone());
    let pending = json!({"id": "p1"});
    followers.relay(0, "prompts", "prompt.pending", &pending);
    assert!(pushed.0.lock().unwrap().is_empty(), "no follower, no push");
    followers.follow("n1", 60_000);
    followers.follow("n2", 100);
    followers.relay(50, "prompts", "prompt.pending", &pending);
    followers.relay(200, "prompts", "prompt.resolved", &pending);
    followers.relay(200, "prompts", "prompt.other", &pending);
    followers.relay(200, "settings", "prompt.pending", &pending);
    followers.relay(200, "prompts", "prompt.pending", &json!({"id": "n9/p1"}));
    followers.relay(
        200,
        "prompts",
        "prompt.pending",
        &json!({"id": "agy-perm:1", "tool": "agy"}),
    );
    assert_eq!(
        *pushed.0.lock().unwrap(),
        vec![
            (
                vec!["n1".to_string(), "n2".to_string()],
                "prompt.pending".to_string()
            ),
            (vec!["n1".to_string()], "prompt.resolved".to_string()),
        ],
        "an expired lease gets nothing; remote, agy and other events never go on"
    );
}

#[test]
fn tell_to_reaches_only_the_nodes_named() {
    let net = MemNet::new();
    let mut home = Opts::named("home");
    home.client = true;
    let seen = Arc::new(Mutex::new(Vec::<(NodeId, String)>::new()));
    struct Spy(Arc<Mutex<Vec<(NodeId, String)>>>);
    impl Hooks for Spy {
        fn peer_prompt(&self, from: &NodeId, event: &str, _data: &Value) {
            self.0
                .lock()
                .unwrap()
                .push((from.clone(), event.to_string()));
        }
    }
    home.hooks = Some(Arc::new(Spy(seen.clone())));
    let (a, b) = pair(&net, home, Opts::named("worker"));
    b.node.tell_to(
        vec![node(7)],
        "prompts",
        "prompt.pending",
        json!({"id": "x"}),
    );
    b.node.tell_to(
        vec![a.id()],
        "prompts",
        "prompt.resolved",
        json!({"id": "y"}),
    );
    eventually("the named node hears it", || {
        !seen.lock().unwrap().is_empty()
    });
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(b.id(), "prompt.resolved".to_string())]
    );
}

#[test]
fn client_republishes_remote_prompt_qualified() {
    let u = unit();
    let source = FakeSource::default();
    following(&u, &source, 1);
    u.prompts
        .on_event(&node(1), "prompt.pending", &prompt("p1"));
    let shown = u.room.named("prompt.pending");
    assert_eq!(shown.len(), 1);
    let p = &shown[0];
    assert_eq!(p["id"], json!(qualify(&node(1), "p1")));
    assert_eq!(p["sessionId"], json!(qualify(&node(1), "s1")));
    assert_eq!(p["node"], json!(node(1)));
    assert_eq!(p["kind"], "permission");
    assert_eq!(p["toolName"], "Bash");
    assert_eq!(p["summary"], "ls");
    assert_eq!(p["input"], json!({"command": "ls"}));
    assert_eq!(p["cwd"], "/w");
    assert_eq!(p["createdMs"], 5);
    assert_eq!(
        p["canAlwaysAllow"], false,
        "always is refused for another node"
    );
    u.prompts
        .on_event(&node(1), "prompt.pending", &prompt("p1"));
    assert_eq!(
        u.room.named("prompt.pending").len(),
        1,
        "a repeat says nothing"
    );
    u.prompts.on_event(
        &node(1),
        "prompt.resolved",
        &json!({"id": "p1", "sessionId": "s1", "outcome": "allow", "by": "app"}),
    );
    let gone = u.room.named("prompt.resolved");
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0]["id"], json!(qualify(&node(1), "p1")));
    assert_eq!(gone[0]["outcome"], "allow");
    assert!(u.prompts.list().is_empty());
}

#[test]
fn a_prompt_from_a_node_not_followed_is_ignored() {
    let u = unit();
    u.prompts
        .on_event(&node(1), "prompt.pending", &prompt("p1"));
    assert!(u.room.named("prompt.pending").is_empty());
}

#[test]
fn remote_prompt_never_carries_owner_transcript_path() {
    let u = unit();
    let source = FakeSource::default();
    following(&u, &source, 1);
    u.prompts
        .on_event(&node(1), "prompt.pending", &prompt("p1"));
    assert_eq!(
        u.room.named("prompt.pending")[0]["transcriptPath"],
        Value::Null
    );

    let mut row = agent_row("s1");
    row["transcriptPath"] = json!("/w/s1.jsonl");
    u.remote.sync(&[peer(1, true)], &OneRow(json!([row])), 0);
    let copy = crate::cluster_mirror::mirror_path(
        &u.dir.path().join("mirror"),
        &node(1),
        &qualify(&node(1), "s1"),
        "/w/s1.jsonl",
    )
    .unwrap();
    std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
    std::fs::write(&copy, b"x").unwrap();
    u.prompts
        .on_event(&node(1), "prompt.pending", &prompt("p2"));
    let shown = u.room.named("prompt.pending");
    assert_eq!(shown[1]["transcriptPath"], json!(copy.to_string_lossy()));
    assert_ne!(shown[1]["transcriptPath"], json!("/w/s1.jsonl"));
}

#[test]
fn hostile_prompt_event_is_dropped() {
    let u = unit();
    let source = FakeSource::default();
    following(&u, &source, 1);
    let mut bad_ids = Vec::new();
    for id in ["a/b", "", "x\u{7}y", &"i".repeat(1_000)] {
        let mut p = prompt("p1");
        p["id"] = json!(id);
        bad_ids.push(p);
    }
    let mut slashed_session = prompt("p1");
    slashed_session["sessionId"] = json!("n2/s9");
    let mut kind = prompt("p1");
    kind["kind"] = json!("shell");
    let mut question = prompt("p1");
    question["question"] = json!({"items": "no"});
    let mut huge_question = prompt("p1");
    huge_question["question"] =
        json!({"items": [{"question": "q".repeat(200_000), "options": []}]});
    for hostile in bad_ids.into_iter().chain([
        slashed_session,
        kind,
        question,
        huge_question,
        json!("text"),
        json!(null),
    ]) {
        u.prompts.on_event(&node(1), "prompt.pending", &hostile);
    }
    assert!(u.room.named("prompt.pending").is_empty());
    assert!(u.prompts.list().is_empty());

    let mut big = prompt("p9");
    big["summary"] = json!("s".repeat(1_000_000));
    big["input"] = json!({"command": "c".repeat(200_000)});
    big["evil"] = json!("field");
    big["node"] = json!("someone else");
    big["transcriptPath"] = json!("/etc/passwd");
    u.prompts.on_event(&node(1), "prompt.pending", &big);
    let shown = &u.room.named("prompt.pending")[0];
    assert_eq!(shown["summary"].as_str().unwrap().chars().count(), TEXT_MAX);
    assert_eq!(shown["input"], json!({}));
    assert!(shown.get("evil").is_none());
    assert_eq!(shown["node"], json!(node(1)));
    assert_eq!(shown["transcriptPath"], Value::Null);

    u.prompts.on_event(
        &node(1),
        "prompt.resolved",
        &json!({"id": "a/b", "sessionId": "s", "outcome": "x", "by": "y"}),
    );
    assert!(u.room.named("prompt.resolved").is_empty());
}

#[test]
fn a_node_may_hold_only_so_many_prompts() {
    let u = unit();
    let source = FakeSource::default();
    following(&u, &source, 1);
    for i in 0..MAX_PER_NODE + 10 {
        u.prompts
            .on_event(&node(1), "prompt.pending", &prompt(&format!("p{i}")));
    }
    assert_eq!(u.prompts.list().len(), MAX_PER_NODE);
}

#[test]
fn clean_pending_keeps_only_known_question_fields() {
    let mut p = prompt("p1");
    p["kind"] = json!("question");
    p["question"] = json!({"items": [{
        "header": "h", "question": "q?", "options": ["a", "b"], "details": null,
        "multiSelect": false, "evil": 1,
    }], "evil": 2});
    let out = clean_pending(&node(1), &p, &|_, _| None).unwrap();
    assert_eq!(
        out["question"],
        json!({"items": [{"header": "h", "question": "q?", "options": ["a", "b"],
                          "details": null, "multiSelect": false}]})
    );
}

#[test]
fn offline_node_prompts_resolve() {
    let u = unit();
    let source = FakeSource::default();
    u.prompts
        .sync(&[peer(1, true), peer(2, true)], true, &source, 0);
    u.prompts
        .on_event(&node(1), "prompt.pending", &prompt("p1"));
    u.prompts
        .on_event(&node(2), "prompt.pending", &prompt("p2"));
    u.prompts
        .sync(&[peer(1, true), peer(2, false)], true, &source, 1_000);
    let resolved = u.room.named("prompt.resolved");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0]["id"], json!(qualify(&node(2), "p2")));
    assert_eq!(resolved[0]["by"], "offline");
    assert_eq!(u.prompts.list().len(), 1);
    u.prompts.sync(&[peer(1, true)], true, &source, 2_000);
    assert_eq!(u.room.named("prompt.resolved").len(), 1, "once");
    u.prompts.sync(&[peer(1, true)], true, &source, 3_000);
    u.prompts
        .sync(&[peer(1, true), peer(2, true)], true, &source, 4_000);
    assert_eq!(
        source
            .lists
            .lock()
            .unwrap()
            .iter()
            .filter(|n| **n == node(2))
            .count(),
        2,
        "a node that comes back is listed again"
    );
}

#[test]
fn a_prompt_already_pending_is_filled_in_from_the_owners_list() {
    let u = unit();
    let source = FakeSource::default();
    source.set(
        &node(1),
        json!([prompt("p1"), {"id": "agy-perm:1", "tool": "agy", "kind": "permission"}]),
    );
    following(&u, &source, 1);
    let list = u.prompts.list();
    assert_eq!(list.len(), 1, "agy prompts stay on their own node");
    assert_eq!(list[0]["id"], json!(qualify(&node(1), "p1")));
    assert_eq!(u.room.named("prompt.pending").len(), 1);
}

#[test]
fn prompts_list_merges_remote_for_app_not_for_node() {
    let u = unit();
    let source = FakeSource::default();
    following(&u, &source, 1);
    u.prompts
        .on_event(&node(1), "prompt.pending", &prompt("p1"));
    let local = json!([{"id": "mine"}]);
    let app = caller(ClientKind::App);
    let merged = u.prompts.merge(local.clone(), &app);
    let ids: Vec<&str> = merged
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["mine", format!("{}/p1", node(1)).as_str()]);
    let relayed = Caller {
        tier: Tier::App,
        client: ClientKind::Node,
        peer: None,
        pid: None,
    };
    assert_eq!(u.prompts.merge(local.clone(), &relayed), local);
}

struct Rig {
    client: TestNode,
    owner: TestNode,
    owner_room: Arc<crate::acting_ops_tests::FakeRoom>,
    client_room: Arc<crate::acting_ops_tests::FakeRoom>,
    handler: Arc<dyn Handler>,
    prompts: Arc<RemotePrompts>,
    _keep: (Arc<LateHandler>, Dir),
}

struct PromptHooks(Arc<RemotePrompts>);

impl Hooks for PromptHooks {
    fn peer_prompt(&self, from: &NodeId, event: &str, data: &Value) {
        self.0.on_event(from, event, data);
    }
}

fn rig() -> Rig {
    let net = MemNet::new();
    let Setup {
        ops: acting, room, ..
    } = setup(true, PROMPT, sessions());
    room.app.store(false, Ordering::SeqCst);
    let handler: Arc<dyn Handler> = Arc::new(Ops::with_acting(
        Arc::new(Mutex::new(sessions())),
        Arc::new(acting),
    ));
    let late = Arc::new(LateHandler::default());
    late.serve(&handler);
    let local: Arc<dyn LocalCalls> = Arc::new(SessionCalls::new(
        DaemonCalls::new(late.clone(), room.clone()),
        Arc::new(HandlerRows::new(late.clone())),
        room.clone(),
    ));
    let client_room = Arc::new(crate::acting_ops_tests::FakeRoom::default());
    client_room.app.store(true, Ordering::SeqCst);
    let dir = Dir::new("prompts-rig");
    let remote = Arc::new(RemoteSessions::new(dir.path().join("mirror")));
    let prompts = Arc::new(RemotePrompts::new(client_room.clone(), remote));
    let mut worker = Opts::named("worker");
    worker.local = Some(local);
    worker.room = Some(room.clone());
    let mut home = Opts::named("home");
    home.client = true;
    home.room = Some(client_room.clone());
    home.hooks = Some(Arc::new(PromptHooks(prompts.clone())));
    let (client, owner) = pair(&net, home, worker);
    let push = Arc::new(NodePush::default());
    push.attach(&owner.node);
    room.attach_prompt_push(push);
    Rig {
        client,
        owner,
        owner_room: room,
        client_room,
        handler,
        prompts,
        _keep: (late, dir),
    }
}

fn hook() -> Caller {
    caller(ClientKind::Hook)
}

fn permission() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hook/permission-request.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

impl Rig {
    fn follow(&self) {
        let source = crate::cluster_sessions::NodeSource::new(self.client.node.clone());
        self.prompts.sync(
            &peers_of(&self.client.node.status()),
            true,
            &source,
            u64::try_from(crate::acting_ops::now_ms()).unwrap(),
        );
        assert!(self
            .owner_room
            .followers
            .active(crate::acting_ops::now_ms()));
    }

    fn ask(&self) -> std::thread::JoinHandle<Value> {
        let handler = self.handler.clone();
        let ask = json!({"event": permission(), "pid": std::process::id(), "waitMs": 20_000});
        std::thread::spawn(move || handler.call(&hook(), ops::HOOK_REQUEST, &ask).unwrap())
    }

    fn pending(&self) -> Value {
        eventually("the prompt reaches the client", || {
            !self.client_room.named("prompt.pending").is_empty()
        });
        self.client_room.named("prompt.pending")[0].clone()
    }

    fn answer(&self, args: Value) -> Result<Value, ApiError> {
        NodeForward::new(self.client.node.clone())
            .forward(&app(), ops::SESSION_PERMISSION, &args)
            .expect("a qualified id is routed")
    }
}

#[test]
fn permission_answer_on_qualified_id_reaches_owner_hook() {
    let rig = rig();
    let nobody = json!({"event": permission(), "pid": std::process::id(), "waitMs": 20_000});
    let out = rig
        .handler
        .call(&hook(), ops::HOOK_REQUEST, &nobody)
        .unwrap();
    assert_eq!(out["action"], "pass", "nobody follows yet: refused at once");
    assert!(rig.client_room.named("prompt.pending").is_empty());

    rig.follow();
    let waiting = rig.ask();
    let shown = rig.pending();
    let qualified = shown["id"].as_str().unwrap().to_string();
    assert!(
        qualified.starts_with(&format!("{}/", rig.owner.id())),
        "{qualified}"
    );
    assert_eq!(shown["canAlwaysAllow"], false);
    let listed = rig.prompts.merge(json!([]), &caller(ClientKind::App));
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert!(!waiting.is_finished(), "the hook waits for an answer");

    let decided = rig
        .answer(json!({"id": qualified, "decision": "allow"}))
        .unwrap();
    assert_eq!(decided["decided"], "allow");
    assert_eq!(waiting.join().unwrap()["action"], "allow");
    eventually("the client hears it resolved", || {
        !rig.client_room.named("prompt.resolved").is_empty()
    });
    assert_eq!(
        rig.client_room.named("prompt.resolved")[0]["id"],
        json!(qualified)
    );
    assert!(rig.prompts.list().is_empty());
    let calls = rig.client.calls.calls.lock().unwrap();
    assert!(calls.is_empty(), "the client ran nothing itself");
}

#[test]
fn forwarded_always_is_refused_or_safe() {
    let rig = rig();
    rig.follow();
    let waiting = rig.ask();
    let shown = rig.pending();
    let qualified = shown["id"].as_str().unwrap().to_string();
    let err = rig
        .answer(json!({"id": qualified, "decision": "always"}))
        .unwrap_err();
    assert!(err.message.contains("always"), "{err:?}");
    assert!(!waiting.is_finished(), "the prompt is still waiting");
    assert!(rig.client_room.named("prompt.resolved").is_empty());
    rig.answer(json!({"id": qualified, "decision": "deny"}))
        .unwrap();
    assert_eq!(waiting.join().unwrap()["action"], "deny");
}

#[test]
fn an_always_from_this_nodes_own_app_is_unchanged() {
    let Setup {
        ops: acting, room, ..
    } = setup(true, PROMPT, sessions());
    let acting = Arc::new(acting);
    let waiting = {
        let acting = acting.clone();
        let ask = json!({"event": permission(), "pid": std::process::id(), "waitMs": 20_000});
        std::thread::spawn(move || acting.call(&hook(), ops::HOOK_REQUEST, &ask).unwrap())
    };
    eventually("held", || !room.named("prompt.pending").is_empty());
    let id = room.named("prompt.pending")[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let relayed = Caller {
        tier: Tier::App,
        client: ClientKind::Node,
        peer: None,
        pid: None,
    };
    let err = acting
        .call(
            &relayed,
            ops::SESSION_PERMISSION,
            &json!({"id": id, "decision": "always"}),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    acting
        .call(
            &app(),
            ops::SESSION_PERMISSION,
            &json!({"id": id, "decision": "always"}),
        )
        .unwrap();
    assert_eq!(waiting.join().unwrap()["action"], "allow");
}

#[test]
fn an_answer_to_a_prompt_that_is_gone_is_an_error_not_a_hang() {
    let rig = rig();
    rig.follow();
    let err = rig
        .answer(json!({"id": format!("{}/nope", rig.owner.id()), "decision": "allow"}))
        .unwrap_err();
    assert!(!err.message.is_empty());
}

#[test]
fn the_follower_thread_follows_while_an_app_is_attached() {
    let rig = rig();
    let follower = PromptFollower::spawn(
        rig.client.node.clone(),
        rig.prompts.clone(),
        rig.client_room.clone() as Arc<dyn Room>,
    );
    eventually("the owner is followed", || {
        rig.owner_room.followers.active(crate::acting_ops::now_ms())
    });
    let waiting = rig.ask();
    let shown = rig.pending();
    rig.answer(json!({"id": shown["id"], "decision": "deny"}))
        .unwrap();
    assert_eq!(waiting.join().unwrap()["action"], "deny");
    follower.close();
    eventually("the follower stops", || rig.prompts.list().is_empty());
}
