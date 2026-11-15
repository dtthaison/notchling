use super::agents;
use super::backend::{BridgeBackend, Devices};
use super::live::{Acting, ForeignLive, LiveBackend, Sources};
use super::model::*;
use super::router::{Router, RouterParts};
use super::test_support::{
    data, error_code, is_ok, MemoryAudit, MemoryCodes, MemoryDevices, MemoryLimiter, RecordingIo,
    NOW_MS,
};
use super::wire::BridgeFailure;
use crate::sessions::agent::sample;
use crate::sessions::testutil::TempDir;
use crate::sessions::{Agent, AgentGroup, AgentTarget, NodeRef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const NODE: &str = "0123456789abcdef";
const REMOTE: &str = "0123456789abcdef/job-7";

fn remote_agent(transcript: Option<String>, online: bool) -> Agent {
    let mut agent = sample(AgentTarget::Job("ab12".into()), transcript.as_deref());
    agent.id = REMOTE.into();
    agent.name = "Build the parser".into();
    agent.group = AgentGroup::Working;
    agent.host = NODE.into();
    agent.node = Some(NodeRef {
        id: NODE.into(),
        name: "mac-mini".into(),
        online,
    });
    agent
}

fn local_agent() -> Agent {
    let mut agent = sample(
        AgentTarget::Job("cd34".into()),
        Some("/h/.claude/projects/p/local-1.jsonl"),
    );
    agent.id = "job-1".into();
    agent
}

fn transcript(dir: &TempDir) -> String {
    let lines = [
        r#"{"type":"user","uuid":"u1","timestamp":"2026-11-01T09:40:00Z","message":{"role":"user","content":"Port the parser"}}"#,
        r#"{"type":"assistant","uuid":"a1","timestamp":"2026-11-01T09:41:00Z","message":{"id":"msg_1","role":"assistant","model":"claude-opus-5-5","content":[{"type":"text","text":"Started on the lexer."}]}}"#,
    ];
    dir.write(
        &format!("cluster/mirror/{NODE}/0a1b2c3d4e5f6a7b.jsonl"),
        &(lines.join("\n") + "\n"),
    )
    .to_string_lossy()
    .into_owned()
}

#[derive(Default)]
struct World {
    agents: Vec<Agent>,
    prompt: Option<PendingPrompt>,
    followed: Mutex<Vec<String>>,
}

impl Sources for Arc<World> {
    fn claude_sessions(&self) -> Vec<Session> {
        let mut sessions = agents::sessions(&self.agents);
        if let Some(prompt) = &self.prompt {
            if let Some(session) = sessions.iter_mut().find(|s| s.id == prompt.session_id) {
                session.pending = Some(prompt.clone());
            }
        }
        sessions
    }
    fn foreign_live(&self, _: &[super::foreign::ForeignRow]) -> ForeignLive {
        ForeignLive::default()
    }
    fn follow_remote(&self, session: &Session) {
        self.followed
            .lock()
            .expect("followed")
            .push(session.id.clone());
    }
    fn now_ms(&self) -> f64 {
        NOW_MS
    }
}

type Sent = (String, String, Option<String>, Control);

#[derive(Default)]
struct Recorded {
    sent: Vec<Sent>,
    responses: Vec<(String, PromptResponse)>,
}

#[derive(Default)]
struct Acts(Arc<Mutex<Recorded>>);

impl Acting for Acts {
    fn send_message(&self, text: &str, session: &Session) -> Result<(), BridgeFailure> {
        self.0.lock().expect("recorded").sent.push((
            session.id.clone(),
            text.into(),
            session.node.as_ref().map(|n| n.id.clone()),
            session.control.clone(),
        ));
        Ok(())
    }
    fn send_now(&self, _: &Session) -> Result<(), BridgeFailure> {
        Ok(())
    }
    fn respond(
        &self,
        prompt: &PendingPrompt,
        response: &PromptResponse,
    ) -> Result<(), BridgeFailure> {
        self.0
            .lock()
            .expect("recorded")
            .responses
            .push((prompt.id.clone(), response.clone()));
        Ok(())
    }
    fn new_session(&self, _: &Path, _: Option<&str>) -> Result<(), BridgeFailure> {
        Ok(())
    }
    fn take_over(&self, _: &Session, _: bool) -> Result<TakeoverOutcome, BridgeFailure> {
        Err(BridgeFailure::invalid("no"))
    }
}

struct Phone {
    router: Router,
    connection: Arc<super::connection::Connection>,
    io: Arc<RecordingIo>,
    recorded: Arc<Mutex<Recorded>>,
    _dir: TempDir,
}

impl Phone {
    fn ask(&self, request: Value) -> Value {
        self.router.handle(&self.connection, &request.to_string());
        self.io.last_json()
    }
}

fn phone(world: &Arc<World>) -> Phone {
    let dir = TempDir::new("bridge-cluster");
    let acts = Acts::default();
    let recorded = acts.0.clone();
    let backend: Arc<dyn BridgeBackend> = Arc::new(LiveBackend::new(world.clone(), acts));
    let devices = Arc::new(MemoryDevices::default());
    let hub = Arc::new(super::hub::Hub::new());
    let router = Router::new(RouterParts {
        backend,
        devices: devices.clone(),
        codes: Arc::new(MemoryCodes::default()),
        limiter: Arc::new(MemoryLimiter::default()),
        audit: Arc::new(MemoryAudit::default()),
        hub: Arc::clone(&hub),
        home: dir.root().to_path_buf(),
        images_dir: dir.path("images"),
        now_ms: Arc::new(|| NOW_MS),
    });
    let (_, token) = Devices::register(devices.as_ref(), "phone", "ios").expect("register");
    let io = Arc::new(RecordingIo::default());
    let connection = hub.connect("192.168.1.20", false, io.clone());
    router.handle(
        &connection,
        &json!({"id": 1, "op": "hello", "token": token}).to_string(),
    );
    Phone {
        router,
        connection,
        io,
        recorded,
        _dir: dir,
    }
}

fn world_with(agents: Vec<Agent>) -> Arc<World> {
    Arc::new(World {
        agents,
        ..World::default()
    })
}

#[test]
fn phone_lists_remote_sessions_with_node() {
    let world = world_with(vec![local_agent(), remote_agent(None, true)]);
    let phone = phone(&world);
    let reply = phone.ask(json!({"id": 2, "op": "sessions.list"}));
    assert!(is_ok(&reply), "{reply}");
    let sessions = data(&reply)["sessions"]
        .as_array()
        .expect("sessions")
        .clone();
    assert_eq!(sessions.len(), 2);
    let local = sessions
        .iter()
        .find(|s| s["id"] == "local-1")
        .expect("local");
    assert!(local.get("node").is_none(), "{local}");
    let remote = sessions.iter().find(|s| s["id"] == REMOTE).expect("remote");
    assert_eq!(
        remote["node"],
        json!({"id": NODE, "name": "mac-mini", "online": true})
    );
    assert_eq!(remote["control"], "job");
    assert_eq!(remote["state"], "running");

    let offline = world_with(vec![remote_agent(None, false)]);
    let reply = self::phone(&offline).ask(json!({"id": 2, "op": "sessions.list"}));
    assert_eq!(data(&reply)["sessions"][0]["node"]["online"], false);
}

#[test]
fn phone_reply_to_remote_session_is_forwarded() {
    let world = world_with(vec![remote_agent(None, true)]);
    let phone = phone(&world);
    let reply = phone.ask(json!({
        "id": 2, "op": "session.message", "sessionId": REMOTE, "text": "keep going"
    }));
    assert!(is_ok(&reply), "{reply}");
    let recorded = phone.recorded.lock().expect("recorded");
    assert_eq!(
        recorded.sent,
        vec![(
            REMOTE.to_string(),
            "keep going".to_string(),
            Some(NODE.to_string()),
            Control::Job(REMOTE.to_string())
        )]
    );
}

#[test]
fn phone_chat_messages_of_remote_session_read_the_mirror() {
    let dir = TempDir::new("bridge-cluster-mirror");
    let mirror = transcript(&dir);
    let world = world_with(vec![remote_agent(Some(mirror), true)]);
    let phone = phone(&world);
    let reply = phone.ask(json!({"id": 2, "op": "chat.get", "sessionId": REMOTE}));
    assert!(is_ok(&reply), "{reply}");
    let texts: Vec<String> = data(&reply)["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|m| m["text"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(texts.iter().any(|t| t == "Port the parser"), "{texts:?}");
    assert_eq!(*world.followed.lock().expect("followed"), vec![REMOTE]);

    let pending = world_with(vec![remote_agent(None, true)]);
    let phone = self::phone(&pending);
    let reply = phone.ask(json!({"id": 2, "op": "chat.get", "sessionId": REMOTE}));
    assert_eq!(error_code(&reply), Some("not_found"), "{reply}");
    assert_eq!(*pending.followed.lock().expect("followed"), vec![REMOTE]);

    let local = world_with(vec![local_agent()]);
    let phone = self::phone(&local);
    phone.ask(json!({"id": 2, "op": "chat.get", "sessionId": "local-1"}));
    assert!(local.followed.lock().expect("followed").is_empty());
}

#[test]
fn phone_never_sends_always_for_a_remote_prompt() {
    let prompt = PendingPrompt {
        id: format!("{NODE}/req-9"),
        session_id: REMOTE.into(),
        kind: PromptKind::Permission,
        tool_name: "Bash".into(),
        summary: "make".into(),
        created_ms: NOW_MS,
    };
    let world = Arc::new(World {
        agents: vec![remote_agent(None, true)],
        prompt: Some(prompt.clone()),
        followed: Mutex::new(Vec::new()),
    });
    let phone = phone(&world);
    let listed = phone.ask(json!({"id": 2, "op": "sessions.list"}));
    let pending = data(&listed)["sessions"][0]["pending"].clone();
    assert_eq!(pending["requestId"], prompt.id.as_str());
    assert!(
        !pending.to_string().to_lowercase().contains("always"),
        "{pending}"
    );

    for allow in [true, false] {
        let reply = phone.ask(json!({
            "id": 3, "op": "prompt.answer", "requestId": prompt.id, "allow": allow
        }));
        assert!(is_ok(&reply), "{reply}");
    }
    let recorded = phone.recorded.lock().expect("recorded");
    assert_eq!(
        recorded.responses,
        vec![
            (prompt.id.clone(), PromptResponse::Allow),
            (prompt.id.clone(), PromptResponse::Deny)
        ]
    );
}

fn read(path: &str) -> Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocol/fixtures");
    serde_json::from_str(&std::fs::read_to_string(root.join(path)).expect("fixture"))
        .expect("fixture json")
}

#[test]
fn old_phone_fixture_without_node_still_parses() {
    use notchling_protocol::generated::roundtrip;
    let old = read("SessionsListResponse.json");
    assert!(old["data"]["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .all(|s| s.get("node").is_none()));
    let parsed = roundtrip("SessionsListResponse", &old)
        .expect("known")
        .expect("parses");
    assert!(parsed["data"]["sessions"][0].get("node").is_none());

    let with = read("SessionsListResponse.node.json");
    let parsed = roundtrip("SessionsListResponse", &with)
        .expect("known")
        .expect("parses");
    assert_eq!(
        parsed["data"]["sessions"][1]["node"],
        json!({"id": NODE, "name": "mac-mini", "online": false})
    );
    assert!(parsed["data"]["sessions"][0].get("node").is_none());
}
