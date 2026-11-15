use super::*;
use crate::chat::outbox::Status;
use crate::testutil::Scratch;
use notchling_engine::outbox::State;
use notchling_engine::transcript::model::{ChatItem, ItemKind};
use notchling_local_api::{
    Allowlist, ApiError, Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle, Tier,
};
use serde_json::json;
use std::thread::JoinHandle;

type Running = (ServerHandle, JoinHandle<std::io::Result<()>>);

struct Daemon {
    rows: Arc<Mutex<Vec<Value>>>,
}

impl Handler for Daemon {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            "sessions.list" => Some(OpSpec::AGENT),
            "outbox.list" => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, _: &Caller, op: &str, _: &Value) -> Result<Value, ApiError> {
        match op {
            "outbox.list" => Ok(Value::Array(
                self.rows.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            )),
            _ => Ok(json!([])),
        }
    }

    fn topics(&self) -> &[&str] {
        &["sessions", "outbox"]
    }

    fn topic_tier(&self, topic: &str) -> Tier {
        if topic == "outbox" {
            Tier::App
        } else {
            Tier::Agent
        }
    }
}

fn wire(id: &str, state: &str, text: &str) -> Value {
    json!({
        "id": id, "session": "ab12cd34", "tool": "claude", "kind": "prompt", "text": text,
        "state": state, "position": 1, "queuedMs": 10_000, "updatedMs": 10_000, "via": "chat",
    })
}

fn serve(socket: &Path, app_tier: bool, rows: &Arc<Mutex<Vec<Value>>>) -> Running {
    let allowlist = if app_tier {
        Allowlist::new([std::env::current_exe().expect("this test binary")])
    } else {
        Allowlist::empty()
    };
    let server = Server::bind(
        ServerConfig {
            socket: socket.to_path_buf(),
            allowlist,
            events: None,
            version: "test".into(),
            limits: Limits::default(),
        },
        Arc::new(Daemon { rows: rows.clone() }),
    )
    .expect("bind");
    server.spawn().expect("serve")
}

fn config(socket: &Path) -> SourceConfig {
    SourceConfig {
        socket: socket.to_path_buf(),
        start: None,
        backoff: Backoff {
            first: Duration::from_millis(20),
            max: Duration::from_millis(80),
        },
        version: "test".into(),
        check_server: false,
    }
}

fn wait_for(source: &SessionSource, what: &str, ok: impl Fn(&View) -> bool) -> View {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let view = source.view();
        if ok(&view) {
            return view;
        }
        assert!(Instant::now() < end, "waited for {what}; saw {view:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_outbox_is_listed_then_followed() {
    let scratch = Scratch::new("sso-follow");
    let socket = scratch.path().join(SOCKET_NAME);
    let rows = Arc::new(Mutex::new(vec![wire("o1", "queued", "first")]));
    let running = serve(&socket, true, &rows);
    let source = SessionSource::start(config(&socket));
    let view = wait_for(&source, "the listed row", |v| {
        !v.outbox.rows("ab12cd34").is_empty()
    });
    assert_eq!(view.outbox.rows("ab12cd34")[0].text, "first");
    let before = source.generation();

    running.0.publish(
        "outbox",
        "outbox.changed",
        &json!({"row": wire("o1", "sending", "first")}),
    );
    let view = wait_for(&source, "the change", |v| {
        v.outbox
            .rows("ab12cd34")
            .first()
            .is_some_and(|r| r.state == State::Sending)
    });
    assert!(view.generation > before);

    running.0.publish(
        "outbox",
        "outbox.removed",
        &json!({"id": "o1", "session": "ab12cd34", "why": "sent"}),
    );
    let view = wait_for(&source, "the removal", |v| {
        v.outbox.rows("ab12cd34").is_empty()
    });
    assert_eq!(view.outbox.bubbles("ab12cd34")[0].status, Status::Sent);

    let shown = Conversation {
        items: vec![ChatItem {
            id: "u1".into(),
            kind: ItemKind::User {
                text: "first".into(),
                images: 0,
                merged: false,
            },
            date: Some(now_ms() as f64 / 1000.0 + 5.0),
        }],
        ..Conversation::default()
    };
    let seen = source.generation();
    source.settle_outbox("ab12cd34", &shown);
    let view = source.view();
    assert!(view.outbox.bubbles("ab12cd34").is_empty());
    assert!(view.generation > seen);
    running.0.shutdown();
    let _ = running.1.join();
}

#[test]
fn a_new_daemon_reseeds_the_outbox_from_its_list() {
    let scratch = Scratch::new("sso-redo");
    let socket = scratch.path().join(SOCKET_NAME);
    let first_rows = Arc::new(Mutex::new(vec![wire("o1", "queued", "one")]));
    let first = serve(&socket, true, &first_rows);
    let source = SessionSource::start(config(&socket));
    wait_for(&source, "the first list", |v| {
        v.outbox.rows("ab12cd34").len() == 1
    });
    first.0.shutdown();
    let _ = first.1.join();
    wait_for(&source, "the fallback", View::daemon_down);
    assert!(source.view().outbox.rows("ab12cd34").is_empty());

    let second_rows = Arc::new(Mutex::new(vec![
        wire("o2", "queued", "two"),
        wire("o3", "queued", "three"),
    ]));
    let second = serve(&socket, true, &second_rows);
    let view = wait_for(&source, "the second list", |v| {
        v.outbox.rows("ab12cd34").len() == 2
    });
    assert_eq!(view.outbox.rows("ab12cd34")[0].id, "o2");
    second.0.shutdown();
    let _ = second.1.join();
}

#[test]
fn a_sent_bubble_survives_a_reconnect_until_the_transcript_shows_it() {
    let scratch = Scratch::new("sso-sent-redo");
    let socket = scratch.path().join(SOCKET_NAME);
    let rows = Arc::new(Mutex::new(vec![wire("o1", "queued", "first")]));
    let first = serve(&socket, true, &rows);
    let source = SessionSource::start(config(&socket));
    wait_for(&source, "the listed row", |v| {
        !v.outbox.rows("ab12cd34").is_empty()
    });
    first.0.publish(
        "outbox",
        "outbox.removed",
        &json!({"id": "o1", "session": "ab12cd34", "why": "sent"}),
    );
    wait_for(&source, "the sent bubble", |v| {
        !v.outbox.bubbles("ab12cd34").is_empty()
    });
    first.0.shutdown();
    let _ = first.1.join();
    wait_for(&source, "the fallback", View::daemon_down);
    assert_eq!(
        source.view().outbox.bubbles("ab12cd34")[0].status,
        Status::Sent
    );

    let again = Arc::new(Mutex::new(vec![wire("o2", "queued", "two")]));
    let second = serve(&socket, true, &again);
    let view = wait_for(&source, "the second list", |v| {
        v.outbox.rows("ab12cd34").len() == 1
    });
    let bubbles = view.outbox.bubbles("ab12cd34");
    assert_eq!(bubbles.len(), 2);
    assert_eq!(bubbles[0].status, Status::Sent);
    assert_eq!(bubbles[0].text, "first");

    let shown = Conversation {
        items: vec![ChatItem {
            id: "u1".into(),
            kind: ItemKind::User {
                text: "first".into(),
                images: 0,
                merged: false,
            },
            date: Some(now_ms() as f64 / 1000.0 + 5.0),
        }],
        ..Conversation::default()
    };
    source.settle_outbox("ab12cd34", &shown);
    let bubbles = source.view().outbox.bubbles("ab12cd34");
    assert_eq!(bubbles.len(), 1);
    assert_eq!(bubbles[0].text, "two");
    second.0.shutdown();
    let _ = second.1.join();
}

#[test]
fn the_agent_tier_follows_no_outbox() {
    let scratch = Scratch::new("sso-agent");
    let socket = scratch.path().join(SOCKET_NAME);
    let rows = Arc::new(Mutex::new(vec![wire("o1", "queued", "first")]));
    let running = serve(&socket, false, &rows);
    let source = SessionSource::start(config(&socket));
    let view = wait_for(&source, "the rows", |v| v.link == Link::Daemon);
    assert!(view.outbox.is_empty());
    running.0.shutdown();
    let _ = running.1.join();
}
