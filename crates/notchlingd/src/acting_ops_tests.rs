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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
pub(crate) struct FakeRoom {
    pub events: Mutex<Vec<(String, Value)>>,
    pub app: AtomicBool,
    pub following: Mutex<Vec<String>>,
    pub followers: crate::cluster_forward::PromptFollowers,
}

impl Room for FakeRoom {
    fn can_answer_prompts(&self) -> bool {
        self.can_answer() || self.followers.active(crate::acting_ops::now_ms())
    }
    fn follow_prompts(&self, node: &str, lease_ms: u64) {
        self.followers
            .follow(node, crate::acting_ops::now_ms() + lease_ms as i64);
    }
    fn attach_prompt_push(&self, push: Arc<dyn crate::cluster_forward::PromptPush>) {
        self.followers.attach(push);
    }
    fn publish(&self, topic: &str, event: &str, data: &Value) {
        self.events
            .lock()
            .unwrap()
            .push((event.to_string(), data.clone()));
        self.followers
            .relay(crate::acting_ops::now_ms(), topic, event, data);
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

pub(crate) type CloseHook = Arc<dyn Fn() + Send + Sync>;

struct FakePty {
    prompt: FakePrompt,
    writes: Arc<Mutex<Vec<String>>>,
    closed: Arc<Mutex<Option<CloseHook>>>,
}

impl Drop for FakePty {
    fn drop(&mut self) {
        let hook = self.closed.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook();
        }
    }
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

pub(crate) type DialogSlot = Arc<Mutex<Option<(Vec<String>, bool)>>>;

pub(crate) type Attaches = Arc<Mutex<Vec<(String, Option<String>)>>>;

#[derive(Default)]
struct FakeAttacher {
    screen: Arc<Mutex<Vec<String>>>,
    attached: Attaches,
    writes: Arc<Mutex<Vec<String>>>,
    deaf: Arc<AtomicBool>,
    starting: Arc<AtomicU64>,
    human: Arc<Mutex<Vec<(u64, String)>>>,
    closed: Arc<Mutex<Option<CloseHook>>>,
    dialog: DialogSlot,
    refuse: Arc<AtomicBool>,
    attaching: Arc<Mutex<Option<CloseHook>>>,
}

impl Attacher for FakeAttacher {
    fn attach(&self, job: &str, config_dir: Option<&str>) -> Result<Box<dyn Pty + Send>, ApiError> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(ApiError::new(ErrorCode::Failed, "no terminal"));
        }
        let hook = self.attaching.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook();
        }
        self.attached
            .lock()
            .unwrap()
            .push((job.to_string(), config_dir.map(String::from)));
        let screen = self.screen.lock().unwrap().clone();
        let lines: Vec<&str> = screen.iter().map(String::as_str).collect();
        let mut prompt = FakePrompt::new(&lines);
        if self.deaf.load(Ordering::SeqCst) {
            prompt = prompt.deaf();
        }
        match self.starting.load(Ordering::SeqCst) {
            0 => {}
            ms => prompt = prompt.starting_for(ms),
        }
        for (at, text) in self.human.lock().unwrap().iter() {
            prompt = prompt.typing(*at, text);
        }
        if let Some((dialog, up)) = self.dialog.lock().unwrap().as_ref() {
            let lines: Vec<&str> = dialog.iter().map(String::as_str).collect();
            prompt = match up {
                true => prompt.with_dialog(&lines),
                false => prompt.opening_dialog_on_command(&lines),
            };
        }
        Ok(Box::new(FakePty {
            prompt,
            writes: self.writes.clone(),
            closed: self.closed.clone(),
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
    pub starting: Arc<AtomicU64>,
    pub human: Arc<Mutex<Vec<(u64, String)>>>,
    pub screen: Arc<Mutex<Vec<String>>>,
    pub closed: Arc<Mutex<Option<CloseHook>>>,
    pub dialog: DialogSlot,
    pub refuse: Arc<AtomicBool>,
    pub attaching: Arc<Mutex<Option<CloseHook>>>,
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
        screen: Arc::new(Mutex::new(screen.iter().map(|s| s.to_string()).collect())),
        ..FakeAttacher::default()
    };
    let shown = attacher.screen.clone();
    let (attached, writes) = (attacher.attached.clone(), attacher.writes.clone());
    let (deaf, starting) = (attacher.deaf.clone(), attacher.starting.clone());
    let human = attacher.human.clone();
    let closed = attacher.closed.clone();
    let dialog = attacher.dialog.clone();
    let refuse = attacher.refuse.clone();
    let attaching = attacher.attaching.clone();
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
        starting,
        human,
        screen: shown,
        closed,
        dialog,
        refuse,
        attaching,
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
    assert_eq!(
        s.ops.spec(ops::SETTINGS_GET_ACTING),
        Some(OpSpec::AGENT_READ)
    );
    assert_eq!(s.ops.spec(ops::SESSION_QUESTION), Some(OpSpec::AGENT_ACTS));
    assert_eq!(s.ops.spec(ops::HOOK_REQUEST), Some(OpSpec::AGENT_HOOK));
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

type Gate = Arc<(Mutex<bool>, std::sync::Condvar)>;
type Order = Arc<Mutex<Vec<(usize, String)>>>;

struct GatedAttacher {
    inner: FakeAttacher,
    hold: &'static str,
    gate: Gate,
    entered: Arc<std::sync::atomic::AtomicUsize>,
    order: Order,
    count: std::sync::atomic::AtomicUsize,
}

struct TaggedPty {
    inner: Box<dyn Pty + Send>,
    n: usize,
    order: Order,
}

impl Pty for TaggedPty {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.order
            .lock()
            .unwrap()
            .push((self.n, String::from_utf8_lossy(bytes).into_owned()));
        std::thread::sleep(Duration::from_millis(3));
        self.inner.write(bytes)
    }
    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        self.inner.wait_quiet(quiet, timeout)
    }
    fn screen(&mut self) -> Vec<String> {
        self.inner.screen()
    }
    fn cursor(&mut self) -> Option<(usize, usize)> {
        self.inner.cursor()
    }
    fn sleep(&mut self, d: Duration) {
        self.inner.sleep(d)
    }
    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }
}

impl Attacher for GatedAttacher {
    fn attach(&self, job: &str, config_dir: Option<&str>) -> Result<Box<dyn Pty + Send>, ApiError> {
        if job == self.hold {
            self.entered.fetch_add(1, Ordering::SeqCst);
            let (open, wake) = &*self.gate;
            let mut open = open.lock().unwrap();
            while !*open {
                open = wake.wait(open).unwrap();
            }
        }
        let n = self.count.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(TaggedPty {
            inner: self.inner.attach(job, config_dir)?,
            n,
            order: self.order.clone(),
        }))
    }
    fn screen_check(&self) -> &dyn ScreenCheck {
        &Markers
    }
}

fn gated(
    hold: &'static str,
) -> (
    Arc<ActingOps>,
    Gate,
    Arc<std::sync::atomic::AtomicUsize>,
    Order,
) {
    let mut list = sessions();
    let second = row("job-idle2", json!({"job": "idle0002"}), "completed", None);
    list.as_array_mut().unwrap().push(second);
    let gate: Gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let entered = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let order = Order::default();
    let inner = FakeAttacher {
        screen: Arc::new(Mutex::new(PROMPT.iter().map(|s| s.to_string()).collect())),
        ..FakeAttacher::default()
    };
    let attacher = GatedAttacher {
        inner,
        hold,
        gate: gate.clone(),
        entered: entered.clone(),
        order: order.clone(),
        count: Default::default(),
    };
    let roots = Roots {
        main_dir: fixture_claude(),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let room = Arc::new(FakeRoom::default());
    room.app.store(true, Ordering::SeqCst);
    let ops = ActingOps::new(
        Arc::new(Acting::in_memory(true)),
        Arc::new(Mutex::new(list)),
        roots,
        room,
        Box::new(attacher),
    );
    (Arc::new(ops), gate, entered, order)
}

fn open_gate(gate: &Gate) {
    let (flag, wake) = &**gate;
    *flag.lock().unwrap() = true;
    wake.notify_all();
}

type Sent = std::sync::mpsc::Receiver<Result<Value, ApiError>>;

fn reply_in_thread(ops: &Arc<ActingOps>, id: &'static str, text: &'static str) -> Sent {
    let (tx, rx) = std::sync::mpsc::channel();
    let ops = ops.clone();
    std::thread::spawn(move || {
        let sent = ops
            .agent(&json!({"id": id}))
            .and_then(|agent| ops.nudge(&agent, text))
            .map(|()| json!({"sent": true}));
        let _ = tx.send(sent);
    });
    rx
}

#[test]
fn locks_are_per_session() {
    let (ops, gate, entered, _order) = gated("idle0001");
    let slow = reply_in_thread(&ops, "job-idle", "to a");
    while entered.load(Ordering::SeqCst) == 0 {
        std::thread::sleep(Duration::from_millis(2));
    }
    let other = reply_in_thread(&ops, "job-idle2", "to b");
    let done = other.recv_timeout(Duration::from_secs(5));
    open_gate(&gate);
    assert_eq!(
        done.expect("b is typed while a's attach hangs").unwrap(),
        json!({"sent": true})
    );
    let slow = slow.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(slow.unwrap(), json!({"sent": true}));
}

fn question_in_thread(ops: &Arc<ActingOps>, id: &'static str) -> Sent {
    let (tx, rx) = std::sync::mpsc::channel();
    let ops = ops.clone();
    std::thread::spawn(move || {
        let _ = tx.send(ops.call(&app(), ops::SESSION_QUESTION, &json!({"id": id})));
    });
    rx
}

#[test]
fn reply_and_question_to_one_session_never_attach_together() {
    let (ops, gate, entered, _order) = gated("idle0001");
    let reply = reply_in_thread(&ops, "job-idle", "to a");
    while entered.load(Ordering::SeqCst) == 0 {
        std::thread::sleep(Duration::from_millis(2));
    }
    let question = question_in_thread(&ops, "job-idle");
    std::thread::sleep(Duration::from_millis(150));
    let inside = entered.load(Ordering::SeqCst);
    open_gate(&gate);
    assert_eq!(inside, 1, "the second op waited for the first one's lock");
    assert!(reply.recv_timeout(Duration::from_secs(5)).unwrap().is_ok());
    assert!(question
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .is_ok());
}

#[test]
fn interrupt_locks_the_job_the_session_moved_to() {
    let (ops, gate, _entered, _order) = gated("none");
    open_gate(&gate);
    let old = ops.typing_locks().for_job("abcd1234");
    let new = ops.typing_locks().for_job("moved001");
    let old_guard = old.lock().unwrap();
    let new_guard = new.lock().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let caller_ops = ops.clone();
    std::thread::spawn(move || {
        let _ = tx.send(caller_ops.call(&app(), ops::SESSION_INTERRUPT, &json!({"id": "job-a"})));
    });
    std::thread::sleep(Duration::from_millis(100));
    {
        let mut list = ops.sessions.lock().unwrap();
        let row = list
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["id"] == "job-a")
            .unwrap();
        row["target"] = json!({"job": "moved001"});
    }
    drop(old_guard);
    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "typed into the new job without its lock"
    );
    drop(new_guard);
    assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap().is_ok());
}

#[test]
fn question_refuses_a_session_that_left_its_job_while_waiting() {
    let (ops, gate, _entered, _order) = gated("none");
    open_gate(&gate);
    let old = ops.typing_locks().for_job("abcd1234");
    let old_guard = old.lock().unwrap();
    let asked = question_in_thread(&ops, "job-a");
    std::thread::sleep(Duration::from_millis(100));
    {
        let mut list = ops.sessions.lock().unwrap();
        let row = list
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["id"] == "job-a")
            .unwrap();
        row["target"] = json!({"process": 4242});
    }
    drop(old_guard);
    let read = asked.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(code(read).0, ErrorCode::BadRequest);
}
