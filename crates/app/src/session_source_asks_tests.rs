use super::*;
use crate::testutil::Scratch;
use notchling_engine::ask::AskState;
use notchling_local_api::{
    Allowlist, ApiError, Caller, Handler, Limits, OpSpec, Server, ServerConfig, Tier,
};
use serde_json::json;

struct Daemon {
    rows: Vec<Value>,
    asks: bool,
}

impl Handler for Daemon {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            "sessions.list" => Some(OpSpec::AGENT_READ),
            "ask.list" if self.asks => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, _: &Caller, op: &str, _: &Value) -> Result<Value, ApiError> {
        match op {
            "ask.list" => Ok(Value::Array(self.rows.clone())),
            _ => Ok(json!([])),
        }
    }

    fn topics(&self) -> &[&str] {
        if self.asks {
            &["sessions", "asks"]
        } else {
            &["sessions"]
        }
    }

    fn topic_tier(&self, topic: &str) -> Tier {
        if topic == "asks" {
            Tier::App
        } else {
            Tier::Agent
        }
    }
}

fn wire(id: &str, state: &str) -> Value {
    json!({
        "id": id, "session": "ab12cd34", "asker": "subagent", "question": "Which one?",
        "choices": ["a", "b"], "mainMayAnswer": false, "mode": "background",
        "state": state, "waiting": true, "createdMs": 10_000, "updatedMs": 10_000,
    })
}

fn serve(
    socket: &Path,
    daemon: Daemon,
) -> (
    notchling_local_api::ServerHandle,
    std::thread::JoinHandle<std::io::Result<()>>,
) {
    Server::bind(
        ServerConfig {
            socket: socket.to_path_buf(),
            allowlist: Allowlist::new([std::env::current_exe().expect("this test binary")]),
            events: None,
            version: "test".into(),
            limits: Limits::default(),
        },
        Arc::new(daemon),
    )
    .expect("bind")
    .spawn()
    .expect("serve")
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

fn job() -> Agent {
    Agent {
        worktree_path: None,
        id: "x".into(),
        name: "job".into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group: notchling_engine::sessions::AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: 1,
        target: notchling_engine::sessions::AgentTarget::Job("ab12cd34".into()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
    }
}

fn open_ids(view: &View) -> Vec<String> {
    let job = job();
    view.asks
        .rows_for(&job)
        .iter()
        .map(|r| r.id.clone())
        .collect()
}

#[test]
fn the_asks_are_listed_then_followed() {
    let scratch = Scratch::new("sso-asks");
    let socket = scratch.path().join(SOCKET_NAME);
    let running = serve(
        &socket,
        Daemon {
            rows: vec![wire("q1", "open")],
            asks: true,
        },
    );
    let source = SessionSource::start(config(&socket));
    let view = wait_for(&source, "the listed question", |v| !open_ids(v).is_empty());
    assert_eq!(open_ids(&view), ["q1"]);
    let before = source.generation();

    running
        .0
        .publish("asks", "ask.changed", &json!({"row": wire("q2", "open")}));
    let view = wait_for(&source, "the new question", |v| open_ids(v).len() == 2);
    assert!(view.generation > before);

    running.0.publish(
        "asks",
        "ask.changed",
        &json!({"row": wire("q1", "dismissed")}),
    );
    let view = wait_for(&source, "the closed question", |v| {
        v.asks
            .rows_for(&job())
            .iter()
            .any(|r| r.state == AskState::Dismissed)
    });
    assert_eq!(open_ids(&view).len(), 2);

    running.0.publish(
        "asks",
        "ask.removed",
        &json!({"id": "q1", "session": "ab12cd34"}),
    );
    let view = wait_for(&source, "the removal", |v| open_ids(v).len() == 1);
    assert_eq!(open_ids(&view), ["q2"]);
}

#[test]
fn a_daemon_without_asks_leaves_the_feed_empty() {
    let scratch = Scratch::new("sso-asks-old");
    let socket = scratch.path().join(SOCKET_NAME);
    let _running = serve(
        &socket,
        Daemon {
            rows: vec![wire("q1", "open")],
            asks: false,
        },
    );
    let source = SessionSource::start(config(&socket));
    let view = wait_for(&source, "the link", |v| v.link == Link::Daemon);
    assert!(open_ids(&view).is_empty());
}
