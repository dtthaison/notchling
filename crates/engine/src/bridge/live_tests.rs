use super::backend::BridgeBackend;
use super::foreign::{self, AcpPermission, ForeignRow, PermissionOption};
use super::live::*;
use super::model::*;
use super::router::{Router, RouterParts};
use super::test_support::{
    error_code, permission, MemoryAudit, MemoryCodes, MemoryDevices, MemoryLimiter, RecordingIo,
    NOW_MS,
};
use super::wire::BridgeFailure;
use crate::catalog::{Catalog, Tool};
use crate::sessions::testutil::TempDir;
use crate::transcript::model::{ChatItem, Conversation, ItemKind, ToolCall, ToolKind};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

fn transcript(dir: &TempDir) -> String {
    let lines = [
        r#"{"type":"user","uuid":"u1","timestamp":"2026-11-01T09:40:00Z","message":{"role":"user","content":"Fix the crash"}}"#,
        r#"{"type":"assistant","uuid":"a1","timestamp":"2026-11-01T09:41:00Z","message":{"id":"msg_1","role":"assistant","model":"claude-opus-5-5","content":[{"type":"text","text":"  The crash was a nil optional.\n"}],"usage":{"input_tokens":50000,"cache_read_input_tokens":1000,"output_tokens":1000}}}"#,
        r#"{"type":"assistant","uuid":"a2","timestamp":"2026-11-01T09:42:00Z","message":{"id":"msg_2","role":"assistant","model":"claude-opus-5-5","content":[{"type":"tool_use","id":"toolu_1","name":"Edit","input":{"file_path":"/src/app/main.swift","old_string":"a","new_string":"b"}}]}}"#,
    ];
    dir.write("projects/app/s1.jsonl", &(lines.join("\n") + "\n"))
        .to_string_lossy()
        .into_owned()
}

fn claude(path: Option<String>, state: SessionState) -> Session {
    let mut s = Session::new("s1", "Fix", "/src/app", Control::ConsolePty, state);
    s.status_updated_ms = NOW_MS;
    s.transcript_path = path;
    s
}

#[test]
fn enrich_adds_activity_the_newest_reply_and_facts() {
    let dir = TempDir::new("bridge-enrich");
    let limits = vec![Limit {
        label: "5h".into(),
        percent: 42.0,
        resets_at: None,
    }];
    let mut session = claude(Some(transcript(&dir)), SessionState::Running);
    let catalog = Catalog::built_in();
    enrich(&mut session, &limits, NOW_MS, &|m| {
        catalog.context_window(Tool::Claude, m)
    });
    assert_eq!(session.activity.as_deref(), Some("Editing main.swift"));
    let last = session.last_message.clone().expect("last message");
    assert_eq!(last.text, "The crash was a nil optional.");
    let facts = session.facts.expect("facts");
    assert_eq!(facts.context_tokens, Some(52_000));
    assert_eq!(facts.context_window, Some(400_000));
    assert_eq!(facts.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(facts.limits, limits);
}

fn no_window(_: &str) -> Option<u64> {
    None
}

#[test]
fn the_phones_context_window_is_the_catalogs() {
    let dir = TempDir::new("bridge-enrich-window");
    let catalog = Catalog::load(&dir.write(
        "catalog.json",
        r#"{"claude": {"windows": {"claude-opus-5-5": 1000000}}}"#,
    ));
    let world = FakeSources {
        claude: vec![claude(Some(transcript(&dir)), SessionState::Running)],
        catalog,
        clock: Mutex::new(NOW_MS),
        ..FakeSources::default()
    };
    let want = world
        .catalog
        .context_window(Tool::Claude, "claude-opus-5-5")
        .map(|w| w as i64);
    assert_eq!(want, Some(1_000_000));
    let backend = LiveBackend::new(world, RecordingActing::default());
    let session = backend
        .sessions()
        .into_iter()
        .find(|s| s.id == "s1")
        .expect("the session");
    assert_eq!(session.facts.expect("facts").context_window, want);
    let none = Catalog::default();
    let mut other = claude(Some(transcript(&dir)), SessionState::Running);
    enrich(&mut other, &[], NOW_MS, &|m| {
        none.context_window(Tool::Claude, m)
    });
    assert_eq!(other.facts.expect("facts").context_window, None);
}

#[test]
fn enrich_leaves_quiet_exited_and_other_tools_alone() {
    let dir = TempDir::new("bridge-enrich-skip");
    let path = Some(transcript(&dir));
    let mut quiet = claude(path.clone(), SessionState::Running);
    quiet.status_updated_ms = NOW_MS - 7.0 * 3_600_000.0;
    let mut exited = claude(path.clone(), SessionState::Exited);
    let mut codex = claude(path.clone(), SessionState::Running);
    codex.tool = "codex".into();
    for session in [&mut quiet, &mut exited, &mut codex] {
        enrich(session, &[], NOW_MS, &no_window);
        assert_eq!(
            (
                session.activity.clone(),
                session.facts.clone(),
                session.last_message.clone()
            ),
            (None, None, None)
        );
    }
    let mut done = claude(path, SessionState::Done);
    enrich(&mut done, &[], NOW_MS, &no_window);
    assert_eq!(
        done.activity, None,
        "only a running session says what it's doing"
    );
    assert!(done.facts.is_some());
}

#[derive(Default)]
struct FakeSources {
    claude: Vec<Session>,
    rows: Vec<ForeignRow>,
    live: ForeignLive,
    clock: Mutex<f64>,
    catalog: Catalog,
}

impl Sources for FakeSources {
    fn claude_sessions(&self) -> Vec<Session> {
        self.claude.clone()
    }
    fn context_window(&self, model: &str) -> Option<u64> {
        self.catalog.context_window(Tool::Claude, model)
    }
    fn takeover_state(&self, _: &Session) -> Option<TakeoverState> {
        Some(TakeoverState::Available)
    }
    fn foreign_rows(&self) -> Vec<ForeignRow> {
        self.rows.clone()
    }
    fn foreign_live(&self, _: &[ForeignRow]) -> ForeignLive {
        self.live.clone()
    }
    fn commands(&self, session: &Session) -> Vec<Command> {
        vec![Command {
            name: format!("for-{}", session.id),
            description: String::new(),
            argument_hint: None,
        }]
    }
    fn now_ms(&self) -> f64 {
        *self.clock.lock().expect("clock")
    }
}

#[derive(Default)]
struct RecordingActing {
    sent: Mutex<Vec<(String, String)>>,
    answered: Mutex<Vec<(String, PromptResponse)>>,
}

impl Acting for RecordingActing {
    fn send_message(&self, text: &str, session: &Session) -> Result<(), BridgeFailure> {
        self.sent
            .lock()
            .expect("sent")
            .push((session.id.clone(), text.into()));
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
        self.answered
            .lock()
            .expect("answered")
            .push((prompt.id.clone(), response.clone()));
        Ok(())
    }
    fn new_session(&self, _: &Path, _: Option<&str>) -> Result<(), BridgeFailure> {
        Ok(())
    }
    fn take_over(&self, session: &Session, _: bool) -> Result<TakeoverOutcome, BridgeFailure> {
        Ok(TakeoverOutcome {
            session_id: session.id.clone(),
            stopped_pid: None,
        })
    }
}

fn agy_world() -> FakeSources {
    let mut held = Session::new(
        "held",
        "Held",
        "/src/app",
        Control::ConsolePty,
        SessionState::NeedsInput,
    );
    held.pending = Some(permission("hook-1", "held"));
    let viewer = Session::new(
        "viewer",
        "Viewer",
        "/src/notes",
        Control::ViewOnly,
        SessionState::Running,
    );
    let row = |id: &str| ForeignRow {
        id: id.into(),
        name: format!("Row {id}"),
        project: "proj".into(),
        cwd: "/src/proj".into(),
        since_ms: NOW_MS - 60_000.0,
        working: false,
        transcript_path: None,
    };
    let permission = AcpPermission {
        rpc_id: "7".into(),
        call: ToolCall::new(
            "t",
            "run_command",
            ToolKind::Shell,
            "Run command".into(),
            "make".into(),
        ),
        options: vec![PermissionOption {
            id: "a1".into(),
            name: "Allow".into(),
            kind: "allow_once".into(),
        }],
    };
    let conversation = Conversation {
        items: vec![
            ChatItem {
                id: "u".into(),
                kind: ItemKind::user("Build it"),
                date: Some(NOW_MS / 1000.0),
            },
            ChatItem {
                id: "a".into(),
                kind: ItemKind::Assistant {
                    text: "Building".into(),
                },
                date: None,
            },
        ],
        ..Conversation::default()
    };
    FakeSources {
        claude: vec![held, viewer],
        rows: vec![row("agy:A1"), row("agy:B2")],
        live: ForeignLive {
            agy_working: HashSet::from(["A1".to_string()]),
            permissions: HashMap::from([("agy:A1".to_string(), permission)]),
            conversations: HashMap::from([("agy:B2".to_string(), conversation)]),
            ..ForeignLive::default()
        },
        clock: Mutex::new(NOW_MS),
        catalog: Catalog::default(),
    }
}

#[test]
fn sessions_list_claude_and_antigravity_rows_with_their_prompts() {
    let backend = LiveBackend::new(agy_world(), RecordingActing::default());
    let sessions = backend.sessions();
    let ids: Vec<_> = sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["held", "viewer", "agy:A1", "agy:B2"]);
    assert_eq!(
        sessions[0].takeover, None,
        "only view-only sessions say whether they can be taken over"
    );
    assert_eq!(sessions[1].takeover, Some(TakeoverState::Available));
    let waiting = &sessions[2];
    assert_eq!(waiting.state, SessionState::NeedsInput);
    let prompt = waiting.pending.clone().expect("permission");
    assert_eq!(prompt.id, "agy-perm:agy:A1:7");
    assert_eq!(
        sessions[3].last_message.as_ref().map(|m| m.text.as_str()),
        Some("Building")
    );

    *backend.sources.clock.lock().expect("clock") = NOW_MS + 5000.0;
    assert_eq!(
        backend.pending_prompt(&prompt.id).map(|p| p.created_ms),
        Some(prompt.created_ms)
    );
    assert_eq!(
        backend.pending_prompt("hook-1").map(|p| p.session_id),
        Some("held".to_string())
    );
    assert_eq!(
        backend.pending_prompt("agy-perm:agy:A1:8"),
        None,
        "another rpc id is gone"
    );

    let chat = backend.chat_messages("agy:B2", None).expect("chat");
    assert_eq!(chat.len(), 2);
    assert_eq!(chat[0].id, foreign::stable_uuid("agy:B2|u"));
    assert_eq!(backend.commands("agy:B2")[0].name, "for-agy:B2");
}

#[test]
fn claude_chat_comes_from_the_transcript_with_the_swift_ids() {
    let dir = TempDir::new("bridge-chat");
    let world = FakeSources {
        claude: vec![claude(Some(transcript(&dir)), SessionState::Running)],
        clock: Mutex::new(NOW_MS),
        ..FakeSources::default()
    };
    let backend = LiveBackend::new(world, ActingOff);
    let chat = backend.chat_messages("s1", None).expect("chat");
    assert!(chat.iter().any(|m| m.text == "Fix the crash"), "{chat:?}");
    assert!(chat
        .iter()
        .all(|m| m.id.len() == 36 && m.id == m.id.to_uppercase()));
    assert_eq!(
        backend.chat_messages("ghost", None).map_err(|e| e.code),
        Err(notchling_protocol::generated::ErrorCode::NotFound)
    );
}

#[test]
fn acting_goes_through_the_acting_trait() {
    let backend = LiveBackend::new(agy_world(), RecordingActing::default());
    let row = backend
        .sessions()
        .into_iter()
        .find(|s| s.id == "agy:A1")
        .expect("row");
    backend.send_message("go on", &row).expect("sent");
    let prompt = backend.pending_prompt("agy-perm:agy:A1:7").expect("prompt");
    backend
        .respond(&prompt, &PromptResponse::Allow)
        .expect("answered");
    assert_eq!(
        *backend.acting.sent.lock().expect("sent"),
        [("agy:A1".to_string(), "go on".to_string())]
    );
    assert_eq!(
        backend.acting.answered.lock().expect("answered")[0].0,
        "agy-perm:agy:A1:7"
    );
}

#[test]
fn with_acting_off_the_router_refuses_and_nothing_is_audited() {
    let backend: Arc<dyn BridgeBackend> = Arc::new(LiveBackend::new(agy_world(), ActingOff));
    let devices = Arc::new(MemoryDevices::default());
    let audit = Arc::new(MemoryAudit::default());
    let hub = Arc::new(super::hub::Hub::new());
    let dir = TempDir::new("bridge-off");
    let router = Router::new(RouterParts {
        backend,
        devices: devices.clone(),
        codes: Arc::new(MemoryCodes::default()),
        limiter: Arc::new(MemoryLimiter::default()),
        audit: audit.clone(),
        hub: Arc::clone(&hub),
        home: dir.root().to_path_buf(),
        images_dir: dir.path("images"),
        now_ms: Arc::new(|| NOW_MS),
    });
    let (_, token) =
        super::backend::Devices::register(devices.as_ref(), "phone", "ios").expect("register");
    let io = Arc::new(RecordingIo::default());
    let connection = hub.connect("192.168.1.20", false, io.clone());
    router.handle(
        &connection,
        &format!(r#"{{"id":1,"op":"hello","token":"{token}"}}"#),
    );
    for request in [
        r#"{"id":2,"op":"session.message","sessionId":"held","text":"hi"}"#,
        r#"{"id":3,"op":"session.sendNow","sessionId":"held"}"#,
        r#"{"id":4,"op":"prompt.answer","requestId":"hook-1","allow":true}"#,
        r#"{"id":5,"op":"session.takeover","sessionId":"viewer"}"#,
    ] {
        router.handle(&connection, request);
        let reply = io.last_json();
        assert_eq!(error_code(&reply), Some("invalid"), "{request}");
        assert_eq!(reply["error"]["message"], ACTING_OFF);
    }
    let cwd = dir.root().to_string_lossy();
    router.handle(
        &connection,
        &format!(r#"{{"id":6,"op":"session.new","cwd":"{cwd}"}}"#),
    );
    assert_eq!(io.last_json()["error"]["message"], ACTING_OFF);
    assert!(audit.recent().is_empty());
    router.handle(&connection, r#"{"id":7,"op":"sessions.list"}"#);
    assert_eq!(
        io.last_json()["data"]["sessions"].as_array().map(Vec::len),
        Some(4)
    );
}

#[test]
fn recent_project_folder_names_decode() {
    let existing: HashSet<&str> = [
        "/Users",
        "/Users/me",
        "/Users/me/my-app",
        "/Users/me/docs",
        "/Users/me/docs/tools",
    ]
    .into();
    let exists = |p: &str| existing.contains(p);
    assert_eq!(
        decode_project_dir("-Users-me-my-app", &exists).as_deref(),
        Some("/Users/me/my-app")
    );
    assert_eq!(
        decode_project_dir("-Users-me-docs-tools", &exists).as_deref(),
        Some("/Users/me/docs/tools")
    );
    assert_eq!(decode_project_dir("-Users-me-gone", &exists), None);
    assert_eq!(decode_project_dir("no-leading-dash", &exists), None);

    let mut old = Session::new(
        "a",
        "a",
        "/Users/me/docs",
        Control::ViewOnly,
        SessionState::Done,
    );
    old.status_updated_ms = 1.0;
    let mut new = Session::new(
        "b",
        "b",
        "/Users/me/my-app",
        Control::ViewOnly,
        SessionState::Done,
    );
    new.status_updated_ms = 2.0;
    let gone = Session::new("c", "c", "/nowhere", Control::ViewOnly, SessionState::Done);
    assert_eq!(
        recent_projects(&[old, gone, new], None, 30, &exists),
        ["/Users/me/my-app", "/Users/me/docs"]
    );
}
