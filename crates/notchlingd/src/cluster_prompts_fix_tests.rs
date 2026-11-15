use super::tests::{following, node, peer, prompt, unit, FakeSource, OneRow, Pushed, Unit};
use super::{PromptSource, RemotePrompts, FOLLOW_EVERY_MS, LEASE_MS};
use crate::cluster_forward::PromptFollowers;
use crate::cluster_prompts_clean::{clean_pending, Found};
use crate::cluster_sessions::agent_row;
use crate::cluster_testkit::{eventually, pair, Opts};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::{qualify, LocalCmd, NodeId};
use notchling_local_api::{ApiError, Caller, ClientKind, Tier};
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

fn ids(u: &Unit) -> Vec<String> {
    u.prompts
        .list()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn merged_list_stays_under_the_line_cap_and_keeps_local_rows() {
    let u = unit();
    let source = Arc::new(FakeSource::default());
    following(&u, &source, 1);
    for i in 0..40 {
        let mut p = prompt(&format!("p{i}"));
        p["input"] = json!({"command": "c".repeat(60_000)});
        u.prompts.on_event(&node(1), "prompt.pending", &p);
    }
    assert!(u.prompts.list().len() > 20);
    let local = json!([{"id": "mine-1"}, {"id": "mine-2"}]);
    let merged = u.prompts.merge(local, &app());
    let size = serde_json::to_vec(&merged).unwrap().len();
    assert!(size < 1024 * 1024, "{size}");
    let rows = merged.as_array().unwrap();
    assert_eq!(rows[0]["id"], "mine-1");
    assert_eq!(rows[1]["id"], "mine-2");
    assert!(rows.len() > 3 && rows.len() < 42, "{}", rows.len());
}

#[test]
fn success_after_lapse_relists() {
    let u = unit();
    let source = Arc::new(FakeSource::default());
    source.set(&node(1), json!([prompt("p1")]));
    following(&u, &source, 1);
    u.prompts
        .on_event(&node(1), "prompt.pending", &prompt("p2"));
    assert_eq!(ids(&u).len(), 2);
    let late = LEASE_MS + FOLLOW_EVERY_MS;
    u.prompts.sync(&[peer(1, true)], true, &source, late);
    assert_eq!(ids(&u), vec![qualify(&node(1), "p1")]);
    let gone: Vec<String> = u
        .room
        .named("prompt.resolved")
        .iter()
        .map(|p| p["by"].as_str().unwrap().to_string())
        .collect();
    assert!(gone.contains(&"unfollowed".to_string()), "{gone:?}");
    assert_eq!(source.lists.lock().unwrap().len(), 2);
}

#[test]
fn lost_resolved_is_reconciled_by_relist() {
    let u = unit();
    let source = Arc::new(FakeSource::default());
    source.set(&node(1), json!([prompt("p1"), prompt("p2")]));
    following(&u, &source, 1);
    assert_eq!(ids(&u).len(), 2);
    source.set(&node(1), json!([prompt("p1")]));
    u.prompts
        .sync(&[peer(1, true)], true, &source, FOLLOW_EVERY_MS);
    assert_eq!(ids(&u), vec![qualify(&node(1), "p1")]);
    let resolved = u.room.named("prompt.resolved");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0]["id"], json!(qualify(&node(1), "p2")));
    assert_eq!(resolved[0]["by"], "unlisted");
}

struct Hooked {
    inner: FakeSource,
    during: Box<dyn Fn() + Send + Sync>,
}

impl PromptSource for Hooked {
    fn follow(&self, node: &NodeId) -> Result<(), ApiError> {
        self.inner.follow(node)
    }

    fn list(&self, node: &NodeId) -> Result<Value, ApiError> {
        let answer = self.inner.list(node);
        (self.during)();
        answer
    }
}

#[test]
fn a_prompt_arriving_while_the_list_is_in_flight_is_kept() {
    let u = unit();
    let prompts = u.prompts.clone();
    let source = Arc::new(Hooked {
        inner: FakeSource::default(),
        during: Box::new(move || prompts.on_event(&node(1), "prompt.pending", &prompt("late"))),
    });
    u.prompts.sync(&[peer(1, true)], true, &source, 0);
    assert_eq!(ids(&u), vec![qualify(&node(1), "late")]);
    source.inner.set(&node(1), json!([prompt("late")]));
    u.prompts
        .sync(&[peer(1, true)], true, &source, FOLLOW_EVERY_MS);
    assert_eq!(ids(&u), vec![qualify(&node(1), "late")]);
}

struct Blocking {
    inner: FakeSource,
    stuck: NodeId,
    release: Mutex<Receiver<()>>,
}

impl PromptSource for Blocking {
    fn follow(&self, node: &NodeId) -> Result<(), ApiError> {
        self.inner.follow(node)?;
        if *node == self.stuck {
            let _ = self.release.lock().unwrap().recv();
        }
        Ok(())
    }

    fn list(&self, node: &NodeId) -> Result<Value, ApiError> {
        self.inner.list(node)
    }
}

#[test]
fn dead_peer_does_not_starve_others() {
    let u = unit();
    let prompts = RemotePrompts::new(u.room.clone(), u.remote.clone());
    let (release, rx) = channel();
    let inner = FakeSource::default();
    inner.set(&node(2), json!([prompt("p1")]));
    let source = Arc::new(Blocking {
        inner,
        stuck: node(1),
        release: Mutex::new(rx),
    });
    let peers = [peer(1, true), peer(2, true)];
    let began = Instant::now();
    prompts.sync(&peers, true, &source, 0);
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "sync never waits for a peer"
    );
    eventually("the healthy owner is followed and listed", || {
        prompts.list().len() == 1
    });
    prompts.sync(&peers, true, &source, 1);
    eventually("the second sync has run", || {
        source.inner.follows.lock().unwrap().len() >= 2
    });
    let follows_of_stuck = source
        .inner
        .follows
        .lock()
        .unwrap()
        .iter()
        .filter(|n| **n == node(1))
        .count();
    assert_eq!(follows_of_stuck, 1, "one ask at a time per peer");
    let _ = release.send(());
}

#[test]
fn oversized_prompt_is_pushed_trimmed() {
    let followers = PromptFollowers::default();
    let pushed = Arc::new(Pushed::default());
    followers.attach(pushed.clone());
    followers.follow("n1", 60_000);
    let mut big = prompt("p1");
    big["input"] = json!({"content": "x".repeat(300_000)});
    followers.relay(0, "prompts", "prompt.pending", &big);
    let sent = pushed.1.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert!(serde_json::to_vec(&sent[0]).unwrap().len() < 128 * 1024);
    assert_eq!(sent[0]["trimmed"], true);
    assert_eq!(sent[0]["id"], "p1");
}

#[test]
fn trimmed_row_is_never_dropped_by_client() {
    let find = |_: &NodeId, _: &str, _: &str| Found::default();
    let mut owner_trimmed = prompt("p1");
    owner_trimmed["input"] = json!({});
    owner_trimmed["trimmed"] = json!(true);
    let out = clean_pending(&node(1), &owner_trimmed, &find).unwrap();
    assert_eq!(out["trimmed"], true);

    let mut question = prompt("p2");
    question["kind"] = json!("question");
    question["question"] = json!({"items": [{"header": "h", "question": "q".repeat(100_000),
                                              "options": ["a"], "multiSelect": false}]});
    assert!(serde_json::to_vec(&question).unwrap().len() < 128 * 1024);
    assert!(clean_pending(&node(1), &question, &find).is_some());

    let mut input = prompt("p3");
    input["input"] = json!({"content": "x".repeat(100_000)});
    let out = clean_pending(&node(1), &input, &find).unwrap();
    assert_eq!(out["input"], json!({}));
    assert_eq!(out["trimmed"], true);
}

#[test]
fn demoted_client_gets_no_push() {
    let net = MemNet::new();
    let mut home = Opts::named("home");
    home.client = true;
    let (a, b) = pair(&net, home, Opts::named("worker"));
    let set = |client: bool| {
        a.node
            .local(LocalCmd::SetMember {
                node: b.id(),
                can_be_master: None,
                client: Some(client),
                reference: None,
                accepts_delegation: None,
                headless: None,
                desired: None,
            })
            .unwrap();
    };
    let leased = vec![b.id().to_string(), "ffffffffffffffff".to_string()];
    assert!(super::leased_clients(&a.node.status(), &leased).is_empty());
    set(true);
    assert_eq!(
        super::leased_clients(&a.node.status(), &leased),
        vec![b.id()]
    );
    set(false);
    assert!(super::leased_clients(&a.node.status(), &leased).is_empty());
}

#[test]
fn remote_prompt_carries_row_id_of_its_session() {
    let u = unit();
    let source = Arc::new(FakeSource::default());
    following(&u, &source, 1);
    let mut by_path = agent_row("job1");
    by_path["transcriptPath"] = json!("/w/s1.jsonl");
    let by_id = agent_row("s2");
    u.remote
        .sync(&[peer(1, true)], &OneRow(json!([by_path, by_id])), 0);

    u.prompts
        .on_event(&node(1), "prompt.pending", &prompt("p1"));
    let mut second = prompt("p2");
    second["sessionId"] = json!("s2");
    second["transcriptPath"] = json!("/elsewhere/x.jsonl");
    u.prompts.on_event(&node(1), "prompt.pending", &second);
    let mut stranger = prompt("p3");
    stranger["sessionId"] = json!("nobody");
    stranger["transcriptPath"] = json!("/none.jsonl");
    u.prompts.on_event(&node(1), "prompt.pending", &stranger);

    let shown = u.room.named("prompt.pending");
    assert_eq!(shown[0]["rowId"], json!(qualify(&node(1), "job1")));
    assert_eq!(shown[1]["rowId"], json!(qualify(&node(1), "s2")));
    assert!(shown[2].get("rowId").is_none());

    u.prompts.on_event(
        &node(1),
        "prompt.resolved",
        &json!({"id": "p1", "sessionId": "s1", "outcome": "allow", "by": "app"}),
    );
    let resolved = u.room.named("prompt.resolved");
    assert_eq!(resolved[0]["rowId"], json!(qualify(&node(1), "job1")));
    u.prompts.on_event(
        &node(1),
        "prompt.resolved",
        &json!({"id": "p3", "sessionId": "nobody", "outcome": "allow", "by": "app"}),
    );
    assert!(u.room.named("prompt.resolved")[1].get("rowId").is_none());
}
