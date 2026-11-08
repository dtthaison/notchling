//! The M3 ops called directly, with a fake room and a fake terminal: the switch, the tiers, what
//! is typed, and the held prompts.

use crate::acting::{Acting, ACTING_OFF};
use crate::acting_ops::{ActingOps, Attacher, Room, ACTING_OPS, CONTROL_OPS};
use notchling_engine::reply::exec::{Markers, Pty, ScreenCheck};
use notchling_engine::reply::keys;
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
pub(crate) struct FakeRoom {
    pub events: Mutex<Vec<(String, Value)>>,
    pub app: AtomicBool,
}

impl Room for FakeRoom {
    fn publish(&self, _topic: &str, event: &str, data: &Value) {
        self.events
            .lock()
            .unwrap()
            .push((event.to_string(), data.clone()));
    }
    fn can_answer(&self) -> bool {
        self.app.load(Ordering::SeqCst)
    }
}

impl FakeRoom {
    fn named(&self, name: &str) -> Vec<Value> {
        let events = self.events.lock().unwrap();
        events
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, d)| d.clone())
            .collect()
    }
}

/// A terminal that shows `screen` and keeps what is written.
struct FakePty {
    screen: Vec<String>,
    writes: Arc<Mutex<Vec<String>>>,
    clock: u64,
}

impl Pty for FakePty {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.writes
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(bytes).into_owned());
        Ok(())
    }
    fn wait_quiet(&mut self, quiet: Duration, _timeout: Duration) -> bool {
        self.clock += quiet.as_millis() as u64;
        true
    }
    fn screen(&mut self) -> Vec<String> {
        self.screen.clone()
    }
    fn sleep(&mut self, d: Duration) {
        self.clock += d.as_millis() as u64;
    }
    fn now_ms(&self) -> u64 {
        self.clock
    }
}

/// Each attach: the job and its config folder.
type Attaches = Arc<Mutex<Vec<(String, Option<String>)>>>;

#[derive(Default)]
struct FakeAttacher {
    screen: Vec<String>,
    attached: Attaches,
    writes: Arc<Mutex<Vec<String>>>,
}

impl Attacher for FakeAttacher {
    fn attach(&self, job: &str, config_dir: Option<&str>) -> Result<Box<dyn Pty>, ApiError> {
        self.attached
            .lock()
            .unwrap()
            .push((job.to_string(), config_dir.map(String::from)));
        Ok(Box::new(FakePty {
            screen: self.screen.clone(),
            writes: self.writes.clone(),
            clock: 0,
        }))
    }
    fn screen_check(&self) -> &dyn ScreenCheck {
        &Markers
    }
}

pub(crate) const SESSION: &str = "6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c";

pub(crate) fn row(id: &str, target: Value, group: &str, transcript: Option<&str>) -> Value {
    json!({
        "id": id, "name": "n", "project": "p", "cwd": "/tmp/demo", "group": group, "label": "",
        "detail": null, "sinceMs": 0, "target": target, "suggestedReply": null,
        "transcriptPath": transcript, "host": "Background", "headless": false,
        "configDir": "/lanes/b", "command": null, "resumeSessionId": null
    })
}

pub(crate) fn sessions() -> Value {
    let transcript = format!("/tmp/none/{SESSION}.jsonl");
    json!([
        row(
            "job-a",
            json!({"job": "abcd1234"}),
            "working",
            Some(&transcript)
        ),
        row("job-idle", json!({"job": "idle0001"}), "completed", None),
        row("term", json!({"process": 4242}), "working", None),
    ])
}

struct Setup {
    ops: ActingOps,
    room: Arc<FakeRoom>,
    attached: Attaches,
    writes: Arc<Mutex<Vec<String>>>,
}

fn setup(on: bool, screen: &[&str], list: Value) -> Setup {
    let room = Arc::new(FakeRoom::default());
    room.app.store(true, Ordering::SeqCst);
    let attacher = FakeAttacher {
        screen: screen.iter().map(|s| s.to_string()).collect(),
        ..FakeAttacher::default()
    };
    let (attached, writes) = (attacher.attached.clone(), attacher.writes.clone());
    let roots = Roots {
        main_dir: PathBuf::from("/nonexistent/claude"),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let ops = ActingOps::new(
        Arc::new(Acting::in_memory(on)),
        Arc::new(Mutex::new(list)),
        roots,
        room.clone(),
        Box::new(attacher),
    );
    Setup {
        ops,
        room,
        attached,
        writes,
    }
}

pub(crate) fn caller(client: ClientKind) -> Caller {
    Caller {
        tier: Tier::App,
        client,
        peer: None,
    }
}

fn app() -> Caller {
    caller(ClientKind::App)
}

fn code(result: Result<Value, ApiError>) -> (ErrorCode, String) {
    let e = result.unwrap_err();
    (e.code, e.message)
}

const PROMPT: &[&str] = &["> ", "  ? for shortcuts"];

#[test]
fn every_acting_op_is_app_tier_and_refused_while_off() {
    let s = setup(false, PROMPT, sessions());
    for op in ACTING_OPS {
        assert_eq!(s.ops.spec(op), Some(OpSpec::APP), "{op}");
        let args = json!({"id": "job-a", "text": "hi", "decision": "allow", "answers": [], "folder": "/tmp"});
        assert_eq!(
            code(s.ops.call(&app(), op, &args)),
            (ErrorCode::Forbidden, ACTING_OFF.to_string()),
            "{op}"
        );
    }
    assert!(s.attached.lock().unwrap().is_empty(), "nothing attached");
    assert!(s.writes.lock().unwrap().is_empty(), "nothing typed");
    assert_eq!(s.ops.spec(ops::SETTINGS_SET_ACTING), Some(OpSpec::APP));
    assert_eq!(s.ops.spec(ops::PROMPTS_LIST), Some(OpSpec::APP));
    for op in [
        ops::SETTINGS_GET_ACTING,
        ops::SESSION_QUESTION,
        ops::HOOK_REQUEST,
    ] {
        assert_eq!(s.ops.spec(op), Some(OpSpec::AGENT), "{op}");
    }
    assert_eq!(s.ops.spec("git.status"), None);
}

#[test]
fn control_ops_wait_for_e11_once_acting_is_on() {
    let s = setup(true, PROMPT, sessions());
    for op in CONTROL_OPS {
        let (c, m) = code(s.ops.call(&app(), op, &json!({"id": "job-a"})));
        assert_eq!(c, ErrorCode::Unimplemented, "{op}");
        assert!(m.contains("E11"), "{m}");
    }
    assert!(s.attached.lock().unwrap().is_empty());
}

#[test]
fn the_switch_is_read_set_and_announced() {
    let s = setup(false, PROMPT, sessions());
    let get = |s: &Setup| s.ops.call(&app(), ops::SETTINGS_GET_ACTING, &json!({}));
    assert_eq!(get(&s).unwrap(), json!({"acting": false}));
    assert_eq!(
        s.ops
            .call(&app(), ops::SETTINGS_SET_ACTING, &json!({"on": true}))
            .unwrap(),
        json!({"acting": true})
    );
    assert_eq!(get(&s).unwrap(), json!({"acting": true}));
    assert_eq!(
        s.room.named("settings.acting"),
        vec![json!({"acting": true})]
    );
    assert_eq!(
        code(
            s.ops
                .call(&app(), ops::SETTINGS_SET_ACTING, &json!({"on": "yes"}))
        )
        .0,
        ErrorCode::BadRequest
    );
}

#[test]
fn a_reply_types_the_plan_into_the_jobs_attach_terminal() {
    let s = setup(true, PROMPT, sessions());
    let out = s.ops.call(
        &app(),
        ops::SESSION_REPLY,
        &json!({"id": "job-a", "text": "hi\x1b[201~\r1"}),
    );
    assert_eq!(out.unwrap(), json!({"sent": true}));
    assert_eq!(
        *s.attached.lock().unwrap(),
        vec![("abcd1234".to_string(), Some("/lanes/b".to_string()))],
        "the lane's config folder goes with the job"
    );
    assert_eq!(
        *s.writes.lock().unwrap(),
        vec![
            keys::clear_input(),
            "\x1b[200~hi[201~\r1\x1b[201~".to_string(),
            "\r".to_string()
        ]
    );
}

#[test]
fn replies_go_only_to_background_jobs_and_never_into_a_dialog() {
    let s = setup(true, PROMPT, sessions());
    let (c, m) = code(s.ops.call(
        &app(),
        ops::SESSION_REPLY,
        &json!({"id": "term", "text": "hi"}),
    ));
    assert_eq!(
        (c, m.as_str()),
        (ErrorCode::BadRequest, "this session takes no replies")
    );
    assert_eq!(
        code(s.ops.call(
            &app(),
            ops::SESSION_REPLY,
            &json!({"id": "nope", "text": "hi"})
        ))
        .0,
        ErrorCode::BadRequest
    );
    assert!(s.attached.lock().unwrap().is_empty());

    let dialog = setup(true, &["Do you want to proceed?", " 1. Yes"], sessions());
    let (c, m) = code(dialog.ops.call(
        &app(),
        ops::SESSION_REPLY,
        &json!({"id": "job-a", "text": "1"}),
    ));
    assert_eq!(c, ErrorCode::Failed);
    assert!(m.starts_with("The session is asking you something"), "{m}");
    assert!(dialog.writes.lock().unwrap().is_empty());
}

#[test]
fn an_interrupt_is_esc_for_a_working_session_only() {
    let s = setup(true, PROMPT, sessions());
    assert_eq!(
        code(
            s.ops
                .call(&app(), ops::SESSION_INTERRUPT, &json!({"id": "job-idle"}))
        )
        .0,
        ErrorCode::BadRequest
    );
    assert!(s.attached.lock().unwrap().is_empty());
    s.ops
        .call(&app(), ops::SESSION_INTERRUPT, &json!({"id": "job-a"}))
        .unwrap();
    assert_eq!(*s.writes.lock().unwrap(), vec!["\x1b".to_string()]);
}

#[test]
fn a_take_back_reads_the_queue_from_the_transcript() {
    let dir = std::env::temp_dir().join(format!("nl-takeback-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let transcript = dir.join(format!("{SESSION}.jsonl"));
    let lines = [
        r#"{"type":"user","message":{"role":"user","content":"first"},"timestamp":"2026-11-05T09:58:00.000Z"}"#,
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-05T09:58:08.490Z","content":"Then say ONE"}"#,
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-05T09:58:11.666Z","content":"And TWO"}"#,
    ];
    std::fs::write(&transcript, lines.join("\n") + "\n").unwrap();
    let path = transcript.to_string_lossy().into_owned();
    let list = json!([row(
        "job-a",
        json!({"job": "abcd1234"}),
        "working",
        Some(&path)
    )]);
    let s = setup(true, PROMPT, list);
    // No popAll is logged (nothing really typed), so nothing was taken: Up's text is cleared.
    let out = s
        .ops
        .call(&app(), ops::SESSION_TAKE_BACK, &json!({"id": "job-a"}))
        .unwrap();
    assert_eq!(out, json!({"taken": false}));
    assert_eq!(
        *s.writes.lock().unwrap(),
        vec![
            keys::clear_input(),
            keys::UP.to_string(),
            keys::clear_input()
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn fixture(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hook")
        .join(name);
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn hook() -> Caller {
    caller(ClientKind::Hook)
}

#[test]
fn only_the_hook_may_ask_and_it_passes_unless_someone_can_answer() {
    let s = setup(true, PROMPT, sessions());
    let ask = json!({"event": fixture("permission-request.json"), "waitMs": 5000});
    for kind in [ClientKind::App, ClientKind::Cli] {
        let (c, _) = code(s.ops.call(&caller(kind), ops::HOOK_REQUEST, &ask));
        assert_eq!(c, ErrorCode::Forbidden, "{kind:?} gets no decision");
    }
    assert!(s.ops.prompts().list().is_empty());
    assert!(s.room.named("prompt.pending").is_empty());

    let other = json!({"event": fixture("other-tool.json")});
    assert_eq!(
        s.ops.call(&hook(), ops::HOOK_REQUEST, &other).unwrap()["action"],
        "pass"
    );
    s.room.app.store(false, Ordering::SeqCst);
    let out = s.ops.call(&hook(), ops::HOOK_REQUEST, &ask).unwrap();
    assert_eq!(
        out,
        json!({"action": "pass", "reason": "no app is attached"})
    );

    let off = setup(false, PROMPT, sessions());
    let out = off.ops.call(&hook(), ops::HOOK_REQUEST, &ask).unwrap();
    assert_eq!(out, json!({"action": "pass", "reason": "acting is off"}));
    assert!(off.room.named("prompt.pending").is_empty());
}

/// Runs a hook request on its own thread and waits for its prompt to be pending.
fn held(s: &Arc<Setup>, event: Value, wait_ms: u64) -> (std::thread::JoinHandle<Value>, String) {
    let ops = s.clone();
    let args = json!({"event": event, "pid": std::process::id(), "waitMs": wait_ms});
    let before = s.room.named("prompt.pending").len();
    let thread =
        std::thread::spawn(move || ops.ops.call(&hook(), ops::HOOK_REQUEST, &args).unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(p) = s.room.named("prompt.pending").get(before) {
            return (thread, p["id"].as_str().unwrap().to_string());
        }
        assert!(std::time::Instant::now() < deadline, "never pending");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_apps_permission_decision_reaches_the_hook() {
    let s = Arc::new(setup(true, PROMPT, sessions()));
    let (thread, id) = held(&s, fixture("permission-request.json"), 10_000);
    let listed = s.ops.call(&app(), ops::PROMPTS_LIST, &json!({})).unwrap();
    assert_eq!(listed[0]["id"], json!(id));
    let out = s
        .ops
        .call(
            &app(),
            ops::SESSION_PERMISSION,
            &json!({"id": id, "decision": "always"}),
        )
        .unwrap();
    assert_eq!(out, json!({"decided": "always", "promptId": id}));
    assert_eq!(
        thread.join().unwrap(),
        json!({"action": "allow", "always": true})
    );
    let resolved = s.room.named("prompt.resolved");
    assert_eq!(resolved[0]["by"], "app");
    assert_eq!(resolved[0]["outcome"], "always");
    // Once only.
    assert!(s
        .ops
        .call(
            &app(),
            ops::SESSION_PERMISSION,
            &json!({"id": id, "decision": "deny"})
        )
        .is_err());
}

#[test]
fn a_session_row_finds_its_held_prompt_and_a_question_takes_answers() {
    let s = Arc::new(setup(true, PROMPT, sessions()));
    let (thread, id) = held(&s, fixture("ask-user-question.json"), 10_000);
    let read = s
        .ops
        .call(
            &caller(ClientKind::Cli),
            ops::SESSION_QUESTION,
            &json!({"id": "job-a"}),
        )
        .unwrap();
    assert_eq!(read["promptId"], json!(id));
    assert_eq!(read["items"][1]["question"], "Where?");
    assert_eq!(
        code(s.ops.call(
            &app(),
            ops::SESSION_PERMISSION,
            &json!({"id": "job-a", "decision": "allow"})
        ))
        .0,
        ErrorCode::BadRequest,
        "a question isn't allowed, it's answered"
    );
    let out = s
        .ops
        .call(
            &app(),
            ops::SESSION_ANSWER,
            &json!({"id": "job-a", "answers": [[1, 3], [2]]}),
        )
        .unwrap();
    assert_eq!(out, json!({"answered": "hook", "promptId": id}));
    assert_eq!(
        thread.join().unwrap(),
        json!({"action": "answer", "answers": {"Which pets?": "Cat, Fish", "Where?": "Away"}})
    );
    assert!(
        s.writes.lock().unwrap().is_empty(),
        "nothing typed for a held prompt"
    );
}

#[test]
fn a_held_hook_passes_at_its_deadline_or_when_the_app_leaves() {
    let s = Arc::new(setup(true, PROMPT, sessions()));
    let (thread, _) = held(&s, fixture("permission-request.json"), 300);
    assert_eq!(
        thread.join().unwrap(),
        json!({"action": "pass", "reason": "timeout"})
    );
    assert_eq!(s.room.named("prompt.resolved")[0]["by"], "timeout");
    assert!(s.ops.prompts().list().is_empty());

    let (thread, _) = held(&s, fixture("permission-request.json"), 10_000);
    s.room.app.store(false, Ordering::SeqCst);
    assert_eq!(thread.join().unwrap()["reason"], "no_app");
}

#[test]
fn a_hook_claude_code_gave_up_on_is_let_go() {
    let s = Arc::new(setup(true, PROMPT, sessions()));
    let ops = s.clone();
    let args = json!({"event": fixture("permission-request.json"), "pid": 99_999_999u64, "waitMs": 10_000});
    let out = ops.ops.call(&hook(), ops::HOOK_REQUEST, &args).unwrap();
    assert_eq!(out["reason"], "hook_gone");
}

#[test]
fn a_question_off_the_screen_waits_for_the_terminal_and_the_switch() {
    let off = setup(false, PROMPT, sessions());
    let read = off.ops.call(
        &caller(ClientKind::Cli),
        ops::SESSION_QUESTION,
        &json!({"id": "job-a"}),
    );
    assert_eq!(code(read), (ErrorCode::Forbidden, ACTING_OFF.to_string()));
    let on = setup(true, PROMPT, sessions());
    let read = on.ops.call(
        &caller(ClientKind::Cli),
        ops::SESSION_QUESTION,
        &json!({"id": "job-a"}),
    );
    assert_eq!(code(read).0, ErrorCode::Unimplemented);
}

#[test]
fn an_answer_with_no_held_prompt_is_typed_page_by_page() {
    // The dialog shows a question the plan doesn't know: only Left is pressed, never a digit.
    let s = setup(true, &["Q: Something?", "Enter to select"], sessions());
    let question = json!({"items": [{"question": "Where?", "options": ["Home", "Away"], "multiSelect": false}]});
    let (c, _) = code(s.ops.call(
        &app(),
        ops::SESSION_ANSWER,
        &json!({"id": "job-a", "answers": [[2]], "question": question}),
    ));
    assert_eq!(c, ErrorCode::Failed);
    let writes = s.writes.lock().unwrap();
    assert!(
        !writes.is_empty() && writes.iter().all(|w| w == keys::LEFT),
        "{writes:?}"
    );
    drop(writes);
    let (c, _) = code(s.ops.call(
        &app(),
        ops::SESSION_ANSWER,
        &json!({"id": "job-a", "answers": [[2]]}),
    ));
    assert_eq!(c, ErrorCode::BadRequest, "no question to answer");
}
