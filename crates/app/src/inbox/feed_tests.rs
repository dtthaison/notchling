use super::*;
use crate::session_source::Backoff;
use crate::testutil::Scratch;
use notchling_local_api::{
    Allowlist, ApiError, Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle, Tier,
    SOCKET_NAME,
};
use std::path::Path;
use std::thread::JoinHandle;

type Running = (ServerHandle, JoinHandle<std::io::Result<()>>);
type Calls = Arc<Mutex<Vec<(String, Value)>>>;

fn item(id: &str, created_ms: i64, state: &str) -> Value {
    json!({
        "id": id,
        "kind": "secret_request",
        "title": format!("Title {id}"),
        "detail": "",
        "source": {},
        "created_ms": created_ms,
        "expires_ms": null,
        "actions": [{"id": "cancel", "label": "Cancel", "primary": false}],
        "state": state,
        "answer": null,
        "answered_by": null,
        "seen": false,
        "open": null
    })
}

struct Daemon {
    calls: Calls,
}

impl Handler for Daemon {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::INBOX_LIST
            | ops::INBOX_ANSWER
            | ops::INBOX_SEEN
            | ops::CONSENT_LIST
            | ops::CONSENT_ANSWER
            | ops::CONSENT_OPENED => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, _: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((op.to_owned(), args.clone()));
        Ok(match op {
            ops::INBOX_LIST => json!({"items": [item("early", 1_000, "open")], "unread": 1}),
            ops::INBOX_ANSWER => json!({"answered": args["id"] == "early"}),
            ops::CONSENT_LIST => json!({"requests": [{"id": "c1", "kind": "setting"}]}),
            ops::CONSENT_ANSWER => json!({"answered": args["decision"] == "allow"}),
            ops::CONSENT_OPENED => json!({"state": "opened"}),
            _ => json!({"unread": 0}),
        })
    }

    fn topics(&self) -> &[&str] {
        &["inbox"]
    }

    fn topic_tier(&self, _: &str) -> Tier {
        Tier::App
    }
}

fn serve(socket: &Path, calls: &Calls) -> Running {
    Server::bind(
        ServerConfig {
            socket: socket.to_path_buf(),
            allowlist: Allowlist::new([std::env::current_exe().expect("this test binary")]),
            events: None,
            version: "test".into(),
            limits: Limits::default(),
        },
        Arc::new(Daemon {
            calls: calls.clone(),
        }),
    )
    .expect("bind")
    .spawn()
    .expect("serve")
}

struct Direct {
    socket: std::path::PathBuf,
}

impl Transport for Direct {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        let options = ConnectOptions {
            check_server: false,
            ..ConnectOptions::default()
        };
        Client::connect_with(&self.socket, ClientKind::App, "test", &options)?.call(op, args)
    }
}

fn start(socket: &Path) -> InboxFeed {
    let config = FeedConfig {
        socket: socket.to_path_buf(),
        version: "test".into(),
        check_server: false,
        backoff: Backoff {
            first: Duration::from_millis(20),
            max: Duration::from_millis(80),
        },
    };
    InboxFeed::start(
        config,
        Arc::new(Direct {
            socket: socket.to_path_buf(),
        }),
    )
}

fn wait_until(what: &str, ok: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < end, "waited for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn ids(feed: &InboxFeed) -> Vec<String> {
    let (needs, done) = feed.snapshot(2_000).sections();
    needs.into_iter().chain(done).map(|r| r.id).collect()
}

#[cfg(unix)]
#[test]
fn it_seeds_from_the_list_then_follows_events_and_answers_by_op() {
    let scratch = Scratch::new("inbox-feed");
    let socket = scratch.path().join(SOCKET_NAME);
    let calls: Calls = Arc::default();
    let daemon = serve(&socket, &calls);
    let feed = start(&socket);
    wait_until("the seed", || ids(&feed) == ["early"]);
    assert_eq!(feed.unread(), 1);

    daemon.0.publish(
        "inbox",
        "inbox.changed",
        &json!({"item": item("late", 3_000, "open")}),
    );
    wait_until("the event", || ids(&feed) == ["late", "early"]);

    daemon
        .0
        .publish("inbox", "inbox.removed", &json!({"id": "early"}));
    wait_until("the removal", || ids(&feed) == ["late"]);

    assert_eq!(feed.answer("late", ActionId::Cancel), Ok(false));
    assert_eq!(feed.answer("early", ActionId::Cancel), Ok(true));
    feed.seen(&["late".to_owned()]).expect("seen");
    assert_eq!(feed.seen(&[]), Ok(()));
    let sent: Vec<(String, Value)> = calls.lock().unwrap().clone();
    assert_eq!(
        sent[2..],
        [
            (
                "inbox.answer".to_owned(),
                json!({"id": "late", "action": "cancel"})
            ),
            (
                "inbox.answer".to_owned(),
                json!({"id": "early", "action": "cancel"})
            ),
            ("inbox.seen".to_owned(), json!({"ids": ["late"]})),
        ]
    );
    assert_eq!(sent[0].0, "inbox.list");
    assert_eq!(sent[1].0, "consent.list");
    daemon.0.shutdown();
    let _ = daemon.1.join();
}

#[test]
fn a_feed_with_no_daemon_shows_nothing_and_refuses_to_send() {
    let feed = InboxFeed::none();
    assert!(!feed.is_live());
    assert_eq!(feed.unread(), 0);
    assert!(feed.answer("a", ActionId::Deny).is_err());
    assert!(feed.seen(&["a".to_owned()]).is_err());
}

#[cfg(unix)]
#[test]
fn a_lost_connection_drops_what_the_daemon_said_and_a_new_one_seeds_again() {
    let scratch = Scratch::new("inbox-lost");
    let socket = scratch.path().join(SOCKET_NAME);
    let calls: Calls = Arc::default();
    let daemon = serve(&socket, &calls);
    let feed = start(&socket);
    wait_until("the seed", || ids(&feed) == ["early"]);
    daemon.0.shutdown();
    let _ = daemon.1.join();
    wait_until("the loss", || ids(&feed).is_empty());
    let _ = std::fs::remove_file(&socket);
    let again = serve(&socket, &calls);
    wait_until("the reseed", || ids(&feed) == ["early"]);
    again.0.shutdown();
    let _ = again.1.join();
}

#[cfg(unix)]
#[test]
fn it_follows_the_consent_requests_and_answers_them_by_the_consent_ops() {
    let scratch = Scratch::new("inbox-cns");
    let socket = scratch.path().join(SOCKET_NAME);
    let calls: Calls = Arc::default();
    let daemon = serve(&socket, &calls);
    let feed = start(&socket);
    wait_until("the seed", || ids(&feed) == ["early"]);
    wait_until("the requests", || feed.snapshot(0).consent("c1").is_some());
    assert!(feed.snapshot(0).consent("c2").is_none());

    let mut raised = item("consent:c1:5", 4_000, "open");
    raised["open"] = json!({"consent": "c1"});
    daemon
        .0
        .publish("inbox", "inbox.changed", &json!({"item": raised}));
    wait_until("the refresh", || {
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(op, _)| op == "consent.list")
            .count()
            == 2
    });

    assert_eq!(feed.consent_answer("c1", true), Ok(true));
    assert_eq!(feed.consent_answer("c1", false), Ok(false));
    assert_eq!(feed.consent_deny_with("c1", "rename it"), Ok(false));
    assert_eq!(feed.consent_opened("c1"), Ok(()));
    let sent: Vec<(String, Value)> = calls.lock().unwrap().clone();
    let last: Vec<_> = sent.iter().rev().take(4).rev().cloned().collect();
    assert_eq!(
        last,
        [
            (
                "consent.answer".to_owned(),
                json!({"consentId": "c1", "decision": "allow"})
            ),
            (
                "consent.answer".to_owned(),
                json!({"consentId": "c1", "decision": "deny"})
            ),
            (
                "consent.answer".to_owned(),
                json!({"consentId": "c1", "decision": "deny", "note": "rename it"})
            ),
            ("consent.opened".to_owned(), json!({"consentId": "c1"})),
        ]
    );
    daemon.0.shutdown();
    let _ = daemon.1.join();
}

const WORKER_ITEM: &str = "0123456789abcdef/consent:c-9:12";

struct Worker {
    calls: Calls,
}

impl Transport for Worker {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((op.to_owned(), args.clone()));
        Ok(match op {
            ops::INBOX_ANSWER => json!({"answered": true}),
            ops::CLUSTER_STATUS => json!({"members": [
                {"id": "0123456789abcdef", "desired": {"acting": false, "bridge": true, "install": false}}
            ]}),
            _ => json!({}),
        })
    }
}

fn worker_feed(calls: &Calls) -> InboxFeed {
    let mut asked = item(WORKER_ITEM, 1_000, "open");
    asked["kind"] = json!("consent");
    asked["switch"] = json!("acting");
    let mut feed = InboxFeed::fixed(InboxModel::from_items(
        vec![serde_json::from_value(asked).unwrap()],
        2_000,
    ));
    feed.transport = Some(Arc::new(Worker {
        calls: calls.clone(),
    }));
    feed
}

#[test]
fn allow_on_a_workers_switch_request_answers_it_then_sets_the_switch_on_that_worker() {
    let calls: Calls = Arc::default();
    let feed = worker_feed(&calls);
    assert_eq!(feed.answer(WORKER_ITEM, ActionId::Allow), Ok(true));
    let sent = calls.lock().unwrap().clone();
    assert_eq!(
        sent,
        [
            (
                ops::INBOX_ANSWER.to_owned(),
                json!({"id": WORKER_ITEM, "action": "allow"})
            ),
            (ops::CLUSTER_STATUS.to_owned(), json!({})),
            (
                ops::CLUSTER_MEMBER_SET.to_owned(),
                json!({
                    "node": "0123456789abcdef",
                    "desired": {"acting": true, "bridge": true, "install": false},
                    "consentId": "c-9",
                })
            ),
        ]
    );
}

#[test]
fn deny_on_a_workers_switch_request_only_answers_it() {
    let calls: Calls = Arc::default();
    let feed = worker_feed(&calls);
    assert_eq!(feed.answer(WORKER_ITEM, ActionId::Deny), Ok(true));
    let sent: Vec<String> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|(op, _)| op.clone())
        .collect();
    assert_eq!(sent, [ops::INBOX_ANSWER.to_owned()]);
}
