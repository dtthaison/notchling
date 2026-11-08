//! The held prompts end to end over an in-process server: the daemon's `prompts.list`,
//! `prompt.pending` and `prompt.resolved` reach the notch's cards through the session source and
//! the notch's source, and a card answers by the prompt's id. The agent tier never sees them and
//! the notch falls back to the Swift hook's `requests/` folder, read-only.

use super::*;
use crate::acting::{Decision, Request};
use crate::notch::model::{Card, NotchModel};
use crate::notch::source::{DataRoot as NotchRoot, Source};
use crate::testutil::Scratch;
use notchling_engine::sessions::{AgentGroup, AgentTarget};
use notchling_local_api::{
    Allowlist, ApiError, Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle, Tier,
};
use serde_json::{json, Value};
use std::thread::JoinHandle;

const SESSION: &str = "6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c";

type Running = (ServerHandle, JoinHandle<std::io::Result<()>>);

/// The daemon's session list and held prompts.
struct Daemon {
    list: Value,
    held: Arc<Mutex<Vec<Value>>>,
}

impl Handler for Daemon {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            "sessions.list" => Some(OpSpec::AGENT),
            "prompts.list" => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, _: &Caller, op: &str, _: &Value) -> Result<Value, ApiError> {
        match op {
            "prompts.list" => Ok(Value::Array(
                self.held.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            )),
            _ => Ok(self.list.clone()),
        }
    }

    fn topics(&self) -> &[&str] {
        &["sessions", "prompts"]
    }

    fn topic_tier(&self, topic: &str) -> Tier {
        if topic == "prompts" {
            Tier::App
        } else {
            Tier::Agent
        }
    }
}

fn row(id: &str) -> Agent {
    Agent {
        id: id.into(),
        name: id.into(),
        project: "p".into(),
        cwd: "/tmp/demo".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: 1_000,
        target: AgentTarget::Job("abcd1234".into()),
        suggested_reply: None,
        transcript_path: Some(format!("/tmp/none/{SESSION}.jsonl")),
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
    }
}

/// What the daemon's `Prompt::to_json` sends for the permission fixture.
fn permission(id: &str) -> Value {
    json!({
        "id": id, "sessionId": SESSION, "kind": "permission", "toolName": "Bash",
        "summary": "rm -rf build", "input": {"command": "rm -rf build"}, "canAlwaysAllow": true,
        "question": null, "cwd": "/tmp/demo", "transcriptPath": "", "createdMs": 2_000
    })
}

fn question(id: &str) -> Value {
    json!({
        "id": id, "sessionId": SESSION, "kind": "question", "toolName": "AskUserQuestion",
        "summary": "Which db?", "input": {}, "canAlwaysAllow": false,
        "question": {"items": [{"header": "DB", "question": "Which db?",
            "options": ["Postgres", "SQLite"], "details": ["", ""], "multiSelect": false}]},
        "cwd": "/tmp/demo", "transcriptPath": "", "createdMs": 3_000
    })
}

fn serve(socket: &Path, app_tier: bool, held: &Arc<Mutex<Vec<Value>>>) -> Running {
    let allowlist = if app_tier {
        Allowlist::new([std::env::current_exe().expect("this test binary")])
    } else {
        Allowlist::empty()
    };
    let server = Server::bind(
        ServerConfig {
            socket: socket.to_path_buf(),
            allowlist,
            audit: None,
            version: "test".into(),
            limits: Limits::default(),
        },
        Arc::new(Daemon {
            list: serde_json::to_value(vec![row("job-a")]).expect("rows"),
            held: held.clone(),
        }),
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

fn prompt_ids(view: &View) -> Vec<String> {
    view.prompts
        .iter()
        .flatten()
        .filter_map(|p| p["id"].as_str().map(String::from))
        .collect()
}

/// One notch poll over what the source has now.
fn poll(notch: &mut Source, model: &mut NotchModel, view: View) -> Option<String> {
    let loaded = notch.load_from(view.agents, view.prompts, 10_000);
    model.update(loaded.agents, loaded.snapshots, loaded.requests)
}

#[test]
fn held_prompts_reach_the_notch_and_leave_when_resolved() {
    let scratch = Scratch::new("ssp-follow");
    let socket = scratch.path().join(SOCKET_NAME);
    // One held before the app connects: listed, not only followed.
    let held = Arc::new(Mutex::new(vec![permission("p1")]));
    let running = serve(&socket, true, &held);
    let source = SessionSource::start(config(&socket));
    let view = wait_for(&source, "the held prompt", |v| prompt_ids(v) == ["p1"]);
    assert_eq!(view.link, Link::Daemon);

    // The notch reads the daemon's prompts, not the Swift `requests/` folder.
    let mut notch = Source::new(NotchRoot::fixture(&scratch.path().join("no-files")));
    let mut model = NotchModel::default();
    assert_eq!(poll(&mut notch, &mut model, view), None, "the baseline");
    let Some(Card::Permission(card)) = model.card("job-a") else {
        panic!("no permission card: {:?}", model.card("job-a"));
    };
    assert_eq!(card.id, "p1", "the card carries the prompt's id");
    assert_eq!(card.summary, "rm -rf build");
    // A held prompt is waiting on you: the row is in Needs Input.
    assert_eq!(model.count(AgentGroup::NeedsInput), 1);
    // Its answer names the prompt, never the row.
    assert_eq!(
        Request::permission(&card.id, Decision::Allow).args,
        json!({"id": "p1", "decision": "allow"})
    );

    // Resolved: the card goes. A new question comes: the notch peeks for it.
    running
        .0
        .publish("prompts", "prompt.resolved", &json!({"id": "p1"}));
    let view = wait_for(&source, "the resolve", |v| prompt_ids(v).is_empty());
    assert_eq!(poll(&mut notch, &mut model, view), None);
    assert_eq!(model.card("job-a"), None);
    running
        .0
        .publish("prompts", "prompt.pending", &question("p2"));
    let view = wait_for(&source, "the question", |v| prompt_ids(v) == ["p2"]);
    assert_eq!(
        poll(&mut notch, &mut model, view).as_deref(),
        Some("job-a"),
        "it peeks"
    );
    let Some(Card::Question(q)) = model.card("job-a") else {
        panic!("no question card");
    };
    assert_eq!(q.items[0].options, ["Postgres", "SQLite"]);
    assert_eq!(model.prompt_id("job-a").as_deref(), Some("p2"));

    // A permission arriving later peeks too.
    running.0.publish(
        "prompts",
        "prompt.resolved",
        &json!({"id": "p2", "outcome": "answer"}),
    );
    let view = wait_for(&source, "the second resolve", |v| prompt_ids(v).is_empty());
    poll(&mut notch, &mut model, view);
    running
        .0
        .publish("prompts", "prompt.pending", &permission("p3"));
    let view = wait_for(&source, "the permission", |v| prompt_ids(v) == ["p3"]);
    assert_eq!(poll(&mut notch, &mut model, view).as_deref(), Some("job-a"));
    running.0.shutdown();
    let _ = running.1.join();
}

#[test]
fn the_agent_tier_follows_no_prompts_and_the_notch_reads_the_files() {
    let scratch = Scratch::new("ssp-agent");
    let socket = scratch.path().join(SOCKET_NAME);
    let held = Arc::new(Mutex::new(vec![permission("p1")]));
    let running = serve(&socket, false, &held);
    let source = SessionSource::start(config(&socket));
    let view = wait_for(&source, "the rows", |v| v.link == Link::Daemon);
    assert_eq!(
        view.prompts, None,
        "not followed: the files are the fallback"
    );
    assert_eq!(view.agents.as_ref().map(Vec::len), Some(1));
    running.0.shutdown();
    let _ = running.1.join();
}
