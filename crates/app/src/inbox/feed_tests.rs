use super::super::system::Detached;
use super::*;
use crate::session_source::Backoff;
use crate::testutil::Scratch;
use notchling_local_api::{
    Allowlist, ApiError, Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle, Tier,
    SOCKET_NAME,
};
use notchling_platform::notify::{Notifier, RecordingNotifier};
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
            ops::INBOX_LIST | ops::INBOX_ANSWER | ops::INBOX_SEEN => Some(OpSpec::APP),
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
            audit: None,
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
    start_with(socket, Arc::new(RecordingNotifier::new()))
}

fn start_with(socket: &Path, notifier: Arc<dyn Notifier>) -> InboxFeed {
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
        notifier,
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
        sent[1..],
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

fn wait_for_banners(notifier: &RecordingNotifier, count: usize) {
    wait_until("the banners", || notifier.sent().len() >= count);
}

#[test]
fn only_an_event_notifies_never_a_seed_or_a_reseed() {
    let scratch = Scratch::new("inbox-banner");
    let socket = scratch.path().join(SOCKET_NAME);
    let calls: Calls = Arc::default();
    let daemon = serve(&socket, &calls);
    let notifier = Arc::new(RecordingNotifier::new());
    let feed = start_with(&socket, Arc::new(Detached::new(notifier.clone())));
    wait_until("the seed", || ids(&feed) == ["early"]);
    std::thread::sleep(Duration::from_millis(100));
    assert!(notifier.sent().is_empty());

    daemon.0.publish(
        "inbox",
        "inbox.changed",
        &json!({"item": item("late", 3_000, "open")}),
    );
    wait_for_banners(&notifier, 1);
    assert_eq!(notifier.sent()[0].title, "Title late");

    daemon.0.shutdown();
    let _ = daemon.1.join();
    wait_until("the loss", || ids(&feed).is_empty());
    let _ = std::fs::remove_file(&socket);
    let again = serve(&socket, &calls);
    wait_until("the reseed", || ids(&feed) == ["early"]);
    again.0.publish(
        "inbox",
        "inbox.changed",
        &json!({"item": item("late", 3_000, "open")}),
    );
    again.0.publish(
        "inbox",
        "inbox.changed",
        &json!({"item": item("later", 4_000, "open")}),
    );
    wait_for_banners(&notifier, 2);
    std::thread::sleep(Duration::from_millis(100));
    let titles: Vec<String> = notifier.sent().into_iter().map(|n| n.title).collect();
    assert_eq!(titles, ["Title late", "Title later"]);
    again.0.shutdown();
    let _ = again.1.join();
}
