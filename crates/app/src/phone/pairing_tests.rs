use super::*;
use crate::phone::approval::{ALLOW, DENY, TITLE};
use crate::testutil::Scratch;
use notchling_local_api::{
    Allowlist, ApiError, Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle, Tier,
};
use std::path::Path;
use std::thread::JoinHandle;

type Running = (ServerHandle, JoinHandle<std::io::Result<()>>);
type Answers = Arc<Mutex<Vec<Value>>>;

struct Daemon {
    answers: Answers,
}

impl Handler for Daemon {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ANSWER_OP).then_some(OpSpec::APP)
    }

    fn call(&self, _: &Caller, _: &str, args: &Value) -> Result<Value, ApiError> {
        let unknown = args["request"] == "pair-gone";
        self.answers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(args.clone());
        Ok(json!({"answered": !unknown}))
    }

    fn topics(&self) -> &[&str] {
        &["bridge"]
    }

    fn topic_tier(&self, _: &str) -> Tier {
        Tier::App
    }
}

fn serve(socket: &Path, answers: &Answers) -> Running {
    Server::bind(
        ServerConfig {
            socket: socket.to_path_buf(),
            allowlist: Allowlist::new([std::env::current_exe().expect("this test binary")]),
            events: None,
            version: "test".into(),
            limits: Limits::default(),
        },
        Arc::new(Daemon {
            answers: answers.clone(),
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

fn start(socket: &Path) -> PairingFeed {
    let config = FeedConfig {
        socket: socket.to_path_buf(),
        version: "test".into(),
        check_server: false,
        backoff: Backoff {
            first: Duration::from_millis(20),
            max: Duration::from_millis(80),
        },
    };
    PairingFeed::start(
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

fn request(id: &str, name: &str) -> Value {
    json!({"request": id, "name": name, "platform": "android", "address": "192.168.1.40"})
}

#[test]
fn the_app_follows_the_bridge_shows_each_request_and_answers_what_the_person_picked() {
    let scratch = Scratch::new("pair-feed");
    let socket = scratch.path().join(SOCKET_NAME);
    let answers: Answers = Arc::default();
    let daemon = serve(&socket, &answers);
    let feed = start(&socket);
    wait_until("the app to follow the bridge", || {
        daemon.0.app_following("bridge") && feed.following()
    });
    assert_eq!(feed.prompt(now_ms()), None);

    daemon.0.publish(
        "bridge",
        "bridge.pair.request",
        &request("pair-1", "Pixel 9"),
    );
    wait_until("the prompt", || feed.prompt(now_ms()).is_some());
    let prompt = feed.prompt(now_ms()).unwrap();
    assert_eq!(prompt.title, TITLE);
    assert_eq!(prompt.request, "pair-1");
    assert!(prompt.message.starts_with("Pixel 9 (android) will see"));
    assert_eq!(prompt.from.as_deref(), Some("Connecting from 192.168.1.40"));
    assert_eq!((prompt.allow, prompt.deny), (ALLOW, DENY));

    feed.answer(&prompt.request, true).unwrap();
    assert_eq!(
        *answers.lock().unwrap(),
        vec![json!({"request": "pair-1", "allow": true})]
    );
    assert_eq!(feed.prompt(now_ms()), None, "answered: the prompt is gone");

    daemon
        .0
        .publish("bridge", "bridge.pair.request", &request("pair-2", "First"));
    daemon.0.publish(
        "bridge",
        "bridge.pair.request",
        &request("pair-3", "Second"),
    );
    wait_until("both requests", || {
        feed.prompt(now_ms())
            .is_some_and(|p| p.more.as_deref() == Some("1 more waiting"))
    });
    let first = feed.prompt(now_ms()).unwrap();
    assert_eq!(first.request, "pair-2");
    feed.answer(&first.request, false).unwrap();
    assert_eq!(
        answers.lock().unwrap().last().unwrap(),
        &json!({"request": "pair-2", "allow": false})
    );
    let second = feed.prompt(now_ms()).unwrap();
    assert_eq!((second.request.as_str(), second.more), ("pair-3", None));

    daemon.0.shutdown();
    let _ = daemon.1.join();
}

#[test]
fn a_request_the_daemon_no_longer_holds_is_answered_without_an_error() {
    let scratch = Scratch::new("pair-gone");
    let socket = scratch.path().join(SOCKET_NAME);
    let answers: Answers = Arc::default();
    let daemon = serve(&socket, &answers);
    let feed = start(&socket);
    wait_until("the follow", || feed.following());
    daemon.0.publish(
        "bridge",
        "bridge.pair.request",
        &request("pair-gone", "Late"),
    );
    wait_until("the prompt", || feed.prompt(now_ms()).is_some());
    assert_eq!(feed.answer("pair-gone", true), Ok(()));
    assert_eq!(feed.prompt(now_ms()), None);
    daemon.0.shutdown();
    let _ = daemon.1.join();
}

#[test]
fn bridge_changed_is_counted_and_off_drops_the_waiting_phones() {
    let scratch = Scratch::new("pair-changed");
    let socket = scratch.path().join(SOCKET_NAME);
    let answers: Answers = Arc::default();
    let daemon = serve(&socket, &answers);
    let feed = start(&socket);
    wait_until("the follow", || feed.following());
    assert_eq!(feed.bridge_changes(), 0);

    daemon
        .0
        .publish("bridge", "bridge.pair.request", &request("pair-1", "Phone"));
    wait_until("the prompt", || feed.prompt(now_ms()).is_some());
    daemon.0.publish(
        "bridge",
        "bridge.changed",
        &json!({"state": "on", "port": 47821}),
    );
    wait_until("the change", || feed.bridge_changes() == 1);
    assert!(feed.prompt(now_ms()).is_some(), "on changes nobody's mind");

    daemon
        .0
        .publish("bridge", "bridge.changed", &json!({"state": "off"}));
    wait_until("the second change", || feed.bridge_changes() == 2);
    assert_eq!(feed.prompt(now_ms()), None);
    daemon.0.shutdown();
    let _ = daemon.1.join();
}

#[test]
fn losing_the_daemon_drops_the_prompts_and_it_follows_again_when_the_daemon_is_back() {
    let scratch = Scratch::new("pair-lost");
    let socket = scratch.path().join(SOCKET_NAME);
    let answers: Answers = Arc::default();
    let daemon = serve(&socket, &answers);
    let feed = start(&socket);
    wait_until("the follow", || feed.following());
    daemon
        .0
        .publish("bridge", "bridge.pair.request", &request("pair-1", "Phone"));
    wait_until("the prompt", || feed.prompt(now_ms()).is_some());
    daemon.0.shutdown();
    let _ = daemon.1.join();
    wait_until("the loss", || !feed.following());
    assert_eq!(feed.prompt(now_ms()), None, "the daemon refused it already");

    let _ = std::fs::remove_file(&socket);
    let back = serve(&socket, &answers);
    wait_until("following again", || {
        feed.following() && back.0.app_following("bridge")
    });
    back.0.shutdown();
    let _ = back.1.join();
}

#[test]
fn a_feed_with_no_daemon_has_nothing_to_show_or_answer() {
    let feed = PairingFeed::none();
    assert!(!feed.is_live() && !feed.following());
    assert_eq!(feed.prompt(now_ms()), None);
    assert!(feed.answer("pair-1", true).is_err());
}
