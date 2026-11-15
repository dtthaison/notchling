use super::tests::{inline, node, peer, prompt, unit, FakeSource};
use super::{
    lock, Job, PromptSource, RemotePrompts, Spawn, FOLLOW_EVERY_MS, MAX_PER_NODE, RETRY_MS,
};
use crate::acting_ops::Room;
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_sessions::RemoteSessions;
use crate::cluster_testkit::{eventually, pair, Dir, Opts};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::{qualify, LocalCmd, NodeId};
use notchling_local_api::ApiError;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

type Hook = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Gate {
    inner: FakeSource,
    on_follow: Mutex<Vec<(NodeId, Hook)>>,
    on_list: Mutex<Vec<(NodeId, Hook)>>,
}

fn take(slot: &Mutex<Vec<(NodeId, Hook)>>, node: &NodeId) -> Option<Hook> {
    let mut hooks = slot.lock().unwrap();
    let at = hooks.iter().position(|(n, _)| n == node)?;
    Some(hooks.remove(at).1)
}

impl Gate {
    fn while_following(&self, node: NodeId, hook: impl FnOnce() + Send + 'static) {
        self.on_follow.lock().unwrap().push((node, Box::new(hook)));
    }

    fn while_listing(&self, node: NodeId, hook: impl FnOnce() + Send + 'static) {
        self.on_list.lock().unwrap().push((node, Box::new(hook)));
    }
}

impl PromptSource for Gate {
    fn follow(&self, node: &NodeId) -> Result<(), ApiError> {
        let answer = self.inner.follow(node);
        if let Some(hook) = take(&self.on_follow, node) {
            hook();
        }
        answer
    }

    fn list(&self, node: &NodeId) -> Result<Value, ApiError> {
        let answer = self.inner.list(node);
        if let Some(hook) = take(&self.on_list, node) {
            hook();
        }
        answer
    }
}

type Tap = Box<dyn Fn(&str, &Value) -> bool + Send>;

#[derive(Default)]
struct HookRoom {
    inner: FakeRoom,
    hook: Mutex<Option<Tap>>,
}

impl Room for HookRoom {
    fn publish(&self, topic: &str, event: &str, data: &Value) {
        self.inner.publish(topic, event, data);
        let hook = self.hook.lock().unwrap().take();
        if let Some(hook) = hook {
            if !hook(event, data) {
                *self.hook.lock().unwrap() = Some(hook);
            }
        }
    }

    fn can_answer(&self) -> bool {
        self.inner.can_answer()
    }

    fn can_follow(&self, topic: &str) -> bool {
        self.inner.can_follow(topic)
    }
}

fn ids(prompts: &RemotePrompts) -> Vec<String> {
    prompts
        .list()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect()
}

fn resolved(id: &str) -> Value {
    json!({"id": id, "sessionId": "s1", "outcome": "allow", "by": "app"})
}

#[test]
fn resolved_during_fill_is_never_followed_by_pending() {
    let room = Arc::new(HookRoom::default());
    let dir = Dir::new("prompts-order");
    let remote = Arc::new(RemoteSessions::new(dir.path().join("mirror")));
    let prompts = RemotePrompts::with_spawn(room.clone(), remote, inline());
    let source = Arc::new(FakeSource::default());
    source.set(&node(1), json!([prompt("p0")]));
    prompts.sync(&[peer(1, true)], true, &source, 0);
    source.set(&node(1), json!([prompt("p1")]));
    let weak = Arc::downgrade(&prompts);
    let p0 = qualify(&node(1), "p0");
    *room.hook.lock().unwrap() = Some(Box::new(move |event, data| {
        if event != "prompt.resolved" || data["id"] != json!(p0) {
            return false;
        }
        if let Some(prompts) = weak.upgrade() {
            prompts.on_event(&node(1), "prompt.resolved", &resolved("p1"));
        }
        true
    }));
    prompts.sync(&[peer(1, true)], true, &source, FOLLOW_EVERY_MS);
    let p1 = json!(qualify(&node(1), "p1"));
    let events: Vec<String> = room
        .inner
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, d)| d["id"] == p1)
        .map(|(e, _)| e.clone())
        .collect();
    assert_eq!(
        events.last().map(String::as_str),
        Some("prompt.resolved"),
        "{events:?}"
    );
    assert!(prompts.list().is_empty());
}

#[test]
fn one_node_drop_does_not_abort_another_nodes_relist() {
    let u = unit();
    let source = Arc::new(Gate::default());
    source.inner.set(&node(1), json!([prompt("p1")]));
    let prompts = u.prompts.clone();
    source.while_listing(node(1), move || prompts.drop_node(&node(2), "offline"));
    u.prompts
        .sync(&[peer(1, true), peer(2, true)], true, &source, 0);
    assert_eq!(ids(&u.prompts), vec![qualify(&node(1), "p1")]);
    let book = lock(&u.prompts.book);
    assert!(!book.followed[&node(1)].listing, "listing is always reset");
}

#[test]
fn follow_completing_after_lapse_relists() {
    let dir = Dir::new("prompts-lapse");
    let room = Arc::new(FakeRoom::default());
    let remote = Arc::new(RemoteSessions::new(dir.path().join("mirror")));
    let clock = Arc::new(AtomicU64::new(0));
    let ticks = clock.clone();
    let prompts = RemotePrompts::with_seams(
        room.clone(),
        remote,
        inline(),
        Arc::new(move || ticks.load(Ordering::SeqCst)),
    );
    let source = Arc::new(Gate::default());
    source.inner.set(&node(1), json!([prompt("p1")]));
    prompts.sync(&[peer(1, true)], true, &source, 0);
    prompts.on_event(&node(1), "prompt.pending", &prompt("p2"));
    let slow = clock.clone();
    source.while_following(node(1), move || {
        slow.fetch_add(10_000, Ordering::SeqCst);
    });
    prompts.sync(&[peer(1, true)], true, &source, 55_000);
    let p2 = room
        .named("prompt.resolved")
        .into_iter()
        .find(|r| r["id"] == json!(qualify(&node(1), "p2")))
        .expect("p2 resolved");
    assert_eq!(p2["by"], "unfollowed", "the lease ended at 60 s");
    assert_eq!(ids(&prompts), vec![qualify(&node(1), "p1")]);
}

#[test]
fn stop_invalidates_in_flight_first_follow() {
    let u = unit();
    let source = Arc::new(Gate::default());
    source.inner.set(&node(1), json!([prompt("p1")]));
    let prompts = u.prompts.clone();
    source.while_following(node(1), move || prompts.stop("unfollowed"));
    u.prompts.sync(&[peer(1, true)], true, &source, 0);
    assert!(u.room.named("prompt.pending").is_empty());
    assert!(u.prompts.list().is_empty());
    assert!(lock(&u.prompts.book)
        .followed
        .get(&node(1))
        .is_none_or(|f| !f.following()));

    let jobs: Arc<Mutex<Vec<Job>>> = Arc::default();
    let queue = jobs.clone();
    let deferred: Spawn = Arc::new(move |job: Job| {
        queue.lock().unwrap().push(job);
        Ok(())
    });
    let later = RemotePrompts::with_spawn(u.room.clone(), u.remote.clone(), deferred);
    let quiet = Arc::new(FakeSource::default());
    later.sync(&[peer(1, true)], true, &quiet, 0);
    later.stop("unfollowed");
    for job in std::mem::take(&mut *jobs.lock().unwrap()) {
        job();
    }
    assert_eq!(quiet.follows(), 0, "a stopped ask never sends node.follow");
    assert!(lock(&later.book).busy.is_empty());
}

#[test]
fn demoted_between_check_and_send_gets_nothing() {
    let net = MemNet::new();
    let mut home = Opts::named("home");
    home.client = true;
    let (a, b) = pair(&net, home, Opts::named("worker"));
    let set = |client: bool| LocalCmd::SetMember {
        node: b.id(),
        can_be_master: None,
        client: Some(client),
        reference: None,
        accepts_delegation: None,
        headless: None,
        desired: None,
    };
    a.node.local(set(true)).unwrap();
    let sent = |id: &str| {
        a.transport
            .sent_lines()
            .iter()
            .any(|l| l.contains(&format!("\"{id}\"")))
    };
    a.node.tell_clients(
        vec![b.id()],
        "prompts",
        "prompt.pending",
        json!({"id": "b17d-fresh"}),
    );
    eventually("a client member is told", || sent("b17d-fresh"));
    a.node.submit().send(set(false));
    a.node.tell_clients(
        vec![b.id()],
        "prompts",
        "prompt.pending",
        json!({"id": "b17d-stale"}),
    );
    a.node.tell_to(
        vec![b.id()],
        "prompts",
        "prompt.resolved",
        json!({"id": "b17d-marker"}),
    );
    eventually("the marker went out", || sent("b17d-marker"));
    assert!(
        !sent("b17d-stale"),
        "the member was demoted before the send"
    );
}

#[test]
fn gone_set_is_bounded() {
    let u = unit();
    let source = Arc::new(Gate::default());
    let prompts = u.prompts.clone();
    let seen = Arc::new(AtomicUsize::new(0));
    let size = seen.clone();
    source.while_listing(node(1), move || {
        for i in 0..MAX_PER_NODE * 12 {
            prompts.on_event(&node(1), "prompt.resolved", &resolved(&format!("junk{i}")));
        }
        let book = lock(&prompts.book);
        size.store(book.followed[&node(1)].gone.len(), Ordering::SeqCst);
    });
    u.prompts.sync(&[peer(1, true)], true, &source, 0);
    let size = seen.load(Ordering::SeqCst);
    assert!(size > 0 && size <= MAX_PER_NODE * 4, "{size}");
}

#[test]
fn spawn_failure_frees_the_peer() {
    let u = unit();
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let flaky: Spawn = Arc::new(move |job: Job| {
        if count.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(std::io::Error::other("no threads"));
        }
        job();
        Ok(())
    });
    let prompts = RemotePrompts::with_spawn(u.room.clone(), u.remote.clone(), flaky);
    let source = Arc::new(FakeSource::default());
    prompts.sync(&[peer(1, true)], true, &source, 0);
    assert!(lock(&prompts.book).busy.is_empty());
    prompts.sync(&[peer(1, true)], true, &source, RETRY_MS - 1);
    assert_eq!(attempts.load(Ordering::SeqCst), 1, "waits RETRY_MS");
    prompts.sync(&[peer(1, true)], true, &source, RETRY_MS);
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(source.follows(), 1);
}
