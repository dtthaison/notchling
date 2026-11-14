use crate::acting::{Acting, ACTING_OFF};
use crate::acting_ops::{ActingOps, Attacher, Room, ACTING_OPS};
use notchling_engine::control::Launcher;
use notchling_engine::reply::exec::{Markers, Pty, ScreenCheck};
use notchling_engine::reply::fake::{FakePrompt, RULE};
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
    pub following: Mutex<Vec<String>>,
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
    fn can_follow(&self, topic: &str) -> bool {
        self.app.load(Ordering::SeqCst) && self.following.lock().unwrap().iter().any(|t| t == topic)
    }
}

impl FakeRoom {
    pub(crate) fn named(&self, name: &str) -> Vec<Value> {
        let events = self.events.lock().unwrap();
        events
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, d)| d.clone())
            .collect()
    }
}

struct FakePty {
    prompt: FakePrompt,
    writes: Arc<Mutex<Vec<String>>>,
}

impl Pty for FakePty {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.writes
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(bytes).into_owned());
        self.prompt.write(bytes)
    }
    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        self.prompt.wait_quiet(quiet, timeout)
    }
    fn screen(&mut self) -> Vec<String> {
        self.prompt.screen()
    }
    fn cursor(&mut self) -> Option<(usize, usize)> {
        self.prompt.cursor()
    }
    fn sleep(&mut self, d: Duration) {
        self.prompt.sleep(d)
    }
    fn now_ms(&self) -> u64 {
        self.prompt.now_ms()
    }
}

type Attaches = Arc<Mutex<Vec<(String, Option<String>)>>>;

#[derive(Default)]
struct FakeAttacher {
    screen: Vec<String>,
    attached: Attaches,
    writes: Arc<Mutex<Vec<String>>>,
    deaf: Arc<AtomicBool>,
}

impl Attacher for FakeAttacher {
    fn attach(&self, job: &str, config_dir: Option<&str>) -> Result<Box<dyn Pty>, ApiError> {
        self.attached
            .lock()
            .unwrap()
            .push((job.to_string(), config_dir.map(String::from)));
        let lines: Vec<&str> = self.screen.iter().map(String::as_str).collect();
        let mut prompt = FakePrompt::new(&lines);
        if self.deaf.load(Ordering::SeqCst) {
            prompt = prompt.deaf();
        }
        Ok(Box::new(FakePty {
            prompt,
            writes: self.writes.clone(),
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

pub(crate) struct Setup {
    pub ops: ActingOps,
    pub room: Arc<FakeRoom>,
    pub attached: Attaches,
    pub writes: Arc<Mutex<Vec<String>>>,
    pub deaf: Arc<AtomicBool>,
}

pub(crate) fn setup(on: bool, screen: &[&str], list: Value) -> Setup {
    let roots = Roots {
        main_dir: fixture_claude(),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    setup_in(on, screen, list, roots, None)
}

pub(crate) fn setup_in(
    on: bool,
    screen: &[&str],
    list: Value,
    roots: Roots,
    launcher: Option<Arc<dyn Launcher>>,
) -> Setup {
    let room = Arc::new(FakeRoom::default());
    room.app.store(true, Ordering::SeqCst);
    let attacher = FakeAttacher {
        screen: screen.iter().map(|s| s.to_string()).collect(),
        ..FakeAttacher::default()
    };
    let (attached, writes) = (attacher.attached.clone(), attacher.writes.clone());
    let deaf = attacher.deaf.clone();
    let me = std::process::id();
    let mut ops = ActingOps::new(
        Arc::new(Acting::in_memory(on)),
        Arc::new(Mutex::new(list)),
        roots,
        room.clone(),
        Box::new(attacher),
    )
    .with_parents(Box::new(move |pid| match pid {
        p if p == me || p == GONE => Some(4242),
        _ => None,
    }));
    if let Some(launcher) = launcher {
        ops = ops.with_launcher(launcher, Box::new(|_| {}));
    }
    Setup {
        ops,
        room,
        attached,
        writes,
        deaf,
    }
}

pub(crate) fn caller(client: ClientKind) -> Caller {
    Caller {
        tier: Tier::App,
        client,
        peer: None,
        pid: Some(std::process::id()),
    }
}

pub(crate) const GONE: u32 = 99_999_999;

fn fixture_claude() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hook/cli/claude")
}

pub(crate) fn app() -> Caller {
    caller(ClientKind::App)
}

pub(crate) fn code(result: Result<Value, ApiError>) -> (ErrorCode, String) {
    let e = result.unwrap_err();
    (e.code, e.message)
}

pub(crate) const PROMPT: &[&str] = &[RULE, "❯ ", RULE, "  ? for shortcuts"];

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
        &json!({"id": "job-idle", "text": "hi\x1b[201~\r1"}),
    );
    assert_eq!(out.unwrap(), json!({"sent": true}));
    assert_eq!(
        *s.attached.lock().unwrap(),
        vec![("idle0001".to_string(), Some("/lanes/b".to_string()))],
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
fn a_reply_the_terminal_keeps_in_its_box_is_an_error_not_a_sent_one() {
    let s = setup(true, PROMPT, sessions());
    s.deaf.store(true, Ordering::SeqCst);
    let (c, m) = code(s.ops.call(
        &app(),
        ops::SESSION_REPLY,
        &json!({"id": "job-idle", "text": "hi"}),
    ));
    assert_eq!(c, ErrorCode::Failed);
    assert!(
        m.starts_with("The message is typed in the session's terminal but wasn't sent"),
        "{m}"
    );
    s.deaf.store(false, Ordering::SeqCst);
    let again = s.ops.call(
        &app(),
        ops::SESSION_REPLY,
        &json!({"id": "job-idle", "text": "hi"}),
    );
    assert_eq!(
        again.unwrap(),
        json!({"sent": true}),
        "a reply that failed holds nothing back"
    );
}

#[test]
fn a_reply_is_typed_only_at_the_prompt() {
    let s = setup(true, &["Resume a session", "  1. fix-hook"], sessions());
    let (c, m) = code(s.ops.call(
        &app(),
        ops::SESSION_REPLY,
        &json!({"id": "job-idle", "text": "hi"}),
    ));
    assert_eq!(c, ErrorCode::Failed);
    assert!(
        m.starts_with("The session's terminal isn't at its prompt"),
        "{m}"
    );
    assert!(s.writes.lock().unwrap().is_empty());
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
        &json!({"id": "job-idle", "text": "1"}),
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
fn a_second_esc_while_stopping_is_not_sent() {
    let s = setup(true, PROMPT, sessions());
    let first = s
        .ops
        .call(&app(), ops::SESSION_INTERRUPT, &json!({"id": "job-a"}))
        .unwrap();
    assert_eq!(first, json!({"sent": true}));
    let second = s
        .ops
        .call(&app(), ops::SESSION_INTERRUPT, &json!({"id": "job-a"}))
        .unwrap();
    assert_eq!(second, json!({"sent": false, "stopping": true}));
    assert_eq!(*s.writes.lock().unwrap(), vec!["\x1b".to_string()]);
    assert_eq!(s.attached.lock().unwrap().len(), 1);
    let s = Arc::new(setup(true, PROMPT, sessions()));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let s = s.clone();
            std::thread::spawn(move || {
                s.ops
                    .call(&app(), ops::SESSION_INTERRUPT, &json!({"id": "job-a"}))
                    .unwrap()
            })
        })
        .collect();
    let sent = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .filter(|r| r["sent"] == true)
        .count();
    assert_eq!(sent, 1);
    assert_eq!(*s.writes.lock().unwrap(), vec!["\x1b".to_string()]);
}

#[test]
fn the_stopping_mark_lasts_twenty_seconds() {
    let s = setup(true, PROMPT, sessions());
    s.ops
        .call(&app(), ops::SESSION_INTERRUPT, &json!({"id": "job-a"}))
        .unwrap();
    s.ops.age_stopping(Duration::from_secs(21));
    assert_eq!(
        s.ops
            .call(&app(), ops::SESSION_INTERRUPT, &json!({"id": "job-a"}))
            .unwrap(),
        json!({"sent": true})
    );
    assert_eq!(s.writes.lock().unwrap().len(), 2);
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

#[test]
fn a_question_off_the_screen_waits_for_the_switch_and_the_app() {
    let screen = [
        "Where should it go?",
        "❯ 1. Home (Recommended)",
        "  2. Away",
        "  3. Type something.",
        "  4. Chat about this",
        "Enter to select · Esc to cancel",
    ];
    let off = setup(false, &screen, sessions());
    let read = off
        .ops
        .call(&app(), ops::SESSION_QUESTION, &json!({"id": "job-a"}));
    assert_eq!(code(read), (ErrorCode::Forbidden, ACTING_OFF.to_string()));
    let on = setup(true, &screen, sessions());
    let agent = Caller {
        tier: Tier::Agent,
        ..caller(ClientKind::Cli)
    };
    let read = on
        .ops
        .call(&agent, ops::SESSION_QUESTION, &json!({"id": "job-a"}));
    assert_eq!(
        code(read).0,
        ErrorCode::Forbidden,
        "the screen moves: app only"
    );
    assert!(on.attached.lock().unwrap().is_empty());
    let read = on
        .ops
        .call(&app(), ops::SESSION_QUESTION, &json!({"id": "job-a"}))
        .unwrap();
    assert_eq!(read["items"][0]["question"], "Where should it go?");
    assert_eq!(
        read["items"][0]["options"],
        json!(["Home (Recommended)", "Away"])
    );
    assert!(read["promptId"].is_null());
    assert!(on.writes.lock().unwrap().is_empty(), "nothing typed");
    let none = setup(true, PROMPT, sessions());
    let read = none
        .ops
        .call(&app(), ops::SESSION_QUESTION, &json!({"id": "job-a"}))
        .unwrap();
    assert_eq!(read, json!({"items": []}));
}

#[test]
fn an_answer_with_no_held_prompt_is_typed_page_by_page() {
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

#[test]
fn prompts_list_trims_an_oversized_prompt_instead_of_failing() {
    let s = setup(true, &[], json!([]));
    let write = |session: &str, file: &str, fill: &str| {
        json!({"hook_event_name": "PermissionRequest", "session_id": session,
            "tool_name": "Write", "cwd": "/w",
            "tool_input": {"file_path": file, "content": fill.repeat(900_000)}})
    };
    let (big, _rx1) = s
        .ops
        .prompts()
        .open(write("s1", "/w/big.txt", "x"), None, 1)
        .unwrap();
    let (_b2, _rx2) = s
        .ops
        .prompts()
        .open(write("s2", "/w/b2.txt", "y"), None, 2)
        .unwrap();
    let small = json!({"hook_event_name": "PermissionRequest", "session_id": "s3",
        "tool_name": "Bash", "cwd": "/w", "tool_input": {"command": "ls"}});
    let (_p, _rx3) = s.ops.prompts().open(small, None, 3).unwrap();
    let list = s
        .ops
        .call(&app(), ops::PROMPTS_LIST, &json!({}))
        .expect("the list still answers");
    let size = serde_json::to_string(&list).unwrap().len();
    assert!(size < 1 << 20, "{size} bytes");
    let rows = list.as_array().unwrap();
    assert_eq!(rows.len(), 3, "every prompt is still there");
    let trimmed = rows.iter().find(|p| p["id"] == json!(big.id)).unwrap();
    assert_eq!(trimmed["trimmed"], true);
    assert_eq!(trimmed["summary"], "/w/big.txt");
}
