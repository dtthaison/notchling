use super::*;
use crate::acting_ops_tests::{app, code, setup, FakeRoom, Setup};
use crate::agy_ops::tests::{self as agy_t};
use notchling_engine::settings::Settings;
use notchling_engine::tools::agy_sidecar::bundle::BUNDLE_ID;
use notchling_engine::tools::agy_sidecar::jobs::Patience;
use notchling_engine::tools::agy_sidecar::{PLUGIN, SIDECAR_FOLDER};
use notchling_local_api::Tier;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

const ID: &str = "457369a9-35a1-40f8-92b9-26b6398b78a1";
const BINARY: &str = "/Applications/Notchling.app/Contents/MacOS/notchling";
const SIDECAR_PID: u32 = 500;
const OTHER_PID: u32 = 600;
const GONE_PID: u32 = 501;
const NEWER_PID: u32 = 502;
const CONFIG: &str = "{\n  \"plugins\": {},\n  \"sidecars\": {\"swarm/board\": {\"projectId\": \"p\", \"schemaVersion\": 1}},\n  \"userSettings\": {\"big\": 12345678901234567890}\n}\n";

struct FakeProcs {
    parents: HashMap<u32, u32>,
    exes: HashMap<u32, PathBuf>,
}

fn fake_app(folder: &str, id: &str) -> PathBuf {
    let app = scratch("bundle").join(folder);
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!(
            "<plist><dict><key>CFBundleExecutable</key><string>Antigravity</string><key>CFBundleIdentifier</key><string>{id}</string></dict></plist>"
        ),
    )
    .unwrap();
    app.join("Contents/MacOS/Antigravity")
}

impl FakeProcs {
    fn standard() -> FakeProcs {
        FakeProcs::under(fake_app("Antigravity.app", BUNDLE_ID))
    }

    fn under(antigravity: PathBuf) -> FakeProcs {
        FakeProcs {
            parents: HashMap::from([
                (SIDECAR_PID, 400),
                (GONE_PID, 400),
                (NEWER_PID, 400),
                (400, 1),
                (OTHER_PID, 300),
                (300, 1),
            ]),
            exes: HashMap::from([
                (SIDECAR_PID, PathBuf::from(BINARY)),
                (NEWER_PID, PathBuf::from(BINARY)),
                (400, antigravity),
                (300, PathBuf::from("/bin/zsh")),
            ]),
        }
    }
}

impl Processes for FakeProcs {
    fn parent(&self, pid: u32) -> Option<u32> {
        self.parents.get(&pid).copied()
    }
    fn name(&self, _: u32) -> Option<String> {
        None
    }
    fn cwd(&self, _: u32) -> Option<PathBuf> {
        None
    }
    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        Some(path.to_path_buf())
    }
    fn exe(&self, pid: u32) -> Option<PathBuf> {
        self.exes.get(&pid).cloned()
    }
}

fn scratch(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "nl-sidecar-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn from(pid: Option<u32>, client: ClientKind) -> Caller {
    Caller {
        tier: Tier::Agent,
        client,
        peer: None,
        pid,
    }
}

fn sidecar() -> Caller {
    from(Some(SIDECAR_PID), ClientKind::Hook)
}

struct Rigged {
    ops: Arc<AgySidecarOps>,
    link: Arc<SidecarLink>,
    _agy: agy_t::Rig,
    acting: Arc<ActingOps>,
    room: Arc<FakeRoom>,
    roots: Roots,
}

fn choose(agy: &agy_t::Rig, channel: &str) {
    let path = scratch("settings").join("settings.json");
    Settings::open(&path)
        .unwrap()
        .set("antigravityChannel", json!(channel))
        .unwrap();
    agy.ops.set_settings(path);
}

fn rigged(channel: Option<&str>, install_on: bool, app_present: bool) -> Rigged {
    let agy = agy_t::rig(true);
    if let Some(channel) = channel {
        choose(&agy, channel);
    }
    let Setup { ops, room, .. } = setup(true, &[], json!([]));
    let acting = Arc::new(ops);
    let home = scratch("home");
    let roots = Roots {
        config: home.join(".gemini/config"),
        data: home.join(".gemini/antigravity"),
        apps: if app_present {
            std::fs::create_dir_all(home.join("Applications/Antigravity.app")).unwrap();
            vec![home.join("Applications/Antigravity.app")]
        } else {
            Vec::new()
        },
    };
    let wait = Duration::from_millis(400);
    let link = SidecarLink::with(
        Box::new(now_ms),
        Patience {
            pickup: wait,
            run: wait,
        },
        Box::new(|_| true),
    );
    agy.ops.attach_sidecar(link.clone());
    let install = SwitchFlag::in_memory("install", "install is off", install_on);
    let ops = Arc::new(AgySidecarOps::new(
        link.clone(),
        agy.ops.clone(),
        acting.clone(),
        install.clone(),
        roots.clone(),
        Some(PathBuf::from(BINARY)),
        Arc::new(FakeProcs::standard()),
    ));
    Rigged {
        ops,
        link,
        _agy: agy,
        acting,
        room,
        roots,
    }
}

fn reply_job(id: &str) -> Job {
    Job {
        id: id.into(),
        kind: Kind::Reply { session: ID.into() },
        text: "hello".into(),
    }
}

#[test]
fn the_lineage_check_wants_antigravity_above_the_caller() {
    let procs = FakeProcs::standard();
    assert!(from_antigravity(&procs, SIDECAR_PID));
    assert!(!from_antigravity(&procs, OTHER_PID));
    assert!(!from_antigravity(&procs, 1));
    assert!(!from_antigravity(&procs, 12345));
}

#[test]
fn a_path_that_only_says_antigravity_is_not_the_app() {
    let other_id = FakeProcs::under(fake_app("Antigravity.app", "com.example.fake"));
    assert!(!from_antigravity(&other_id, SIDECAR_PID));
    let ide = FakeProcs::under(fake_app(
        "Antigravity IDE.app",
        "com.google.antigravity-ide",
    ));
    assert!(!from_antigravity(&ide, SIDECAR_PID));
    let no_bundle = FakeProcs::under(PathBuf::from("/tmp/antigravity/Antigravity"));
    assert!(!from_antigravity(&no_bundle, SIDECAR_PID));
    let moved = FakeProcs::under(fake_app("Somewhere/Renamed.app", BUNDLE_ID));
    assert!(from_antigravity(&moved, SIDECAR_PID));
}

#[test]
fn only_a_hook_client_that_antigravity_started_may_use_the_sidecar_ops() {
    let r = rigged(Some("sidecar"), true, true);
    let calls = [
        (ops::AGY_SIDECAR_TAKE, json!({"waitMs": 10})),
        (
            ops::AGY_SIDECAR_REPORT,
            json!({"id": "x", "result": "failed"}),
        ),
        (
            ops::AGY_HOOK,
            json!({"event": "pre-invocation", "input": "{}"}),
        ),
    ];
    let strangers = [
        from(Some(SIDECAR_PID), ClientKind::Cli),
        from(Some(SIDECAR_PID), ClientKind::App),
        from(Some(OTHER_PID), ClientKind::Hook),
        from(None, ClientKind::Hook),
    ];
    for (op, args) in &calls {
        for stranger in &strangers {
            let (kind, _) = code(r.ops.call(stranger, op, args));
            assert_eq!(kind, ErrorCode::Forbidden, "{op} {stranger:?}");
        }
        assert!(r.ops.call(&sidecar(), op, args).is_ok(), "{op}");
    }
    let imposter = AgySidecarOps::new(
        r.link.clone(),
        r._agy.ops.clone(),
        r.acting.clone(),
        SwitchFlag::in_memory("install", "install is off", true),
        r.roots.clone(),
        Some(PathBuf::from(BINARY)),
        Arc::new(FakeProcs::under(fake_app(
            "Antigravity.app",
            "com.example.fake",
        ))),
    );
    for (op, args) in &calls {
        let (kind, _) = code(imposter.call(&sidecar(), op, args));
        assert_eq!(kind, ErrorCode::Forbidden, "{op}");
    }
    assert_eq!(r.ops.spec(ops::AGY_SIDECAR_TAKE), Some(OpSpec::AGENT_READ));
    assert_eq!(r.ops.spec(ops::AGY_SIDECAR_SETUP), Some(OpSpec::APP));
    assert_eq!(r.ops.spec(ops::AGY_SEND), None);
}

#[test]
fn a_job_is_handed_over_once_and_the_sidecar_counts_as_running_while_it_asks() {
    let r = rigged(Some("sidecar"), true, true);
    assert!(!r.link.running());
    r.link.queue().submit(reply_job("a")).unwrap();
    let out = r
        .ops
        .call(&sidecar(), ops::AGY_SIDECAR_TAKE, &json!({"waitMs": 10}))
        .unwrap();
    assert_eq!(out["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(out["jobs"][0]["id"], "a");
    assert_eq!(out["jobs"][0]["session"], ID);
    assert!(r.link.running());
    let again = r
        .ops
        .call(&sidecar(), ops::AGY_SIDECAR_TAKE, &json!({"waitMs": 10}))
        .unwrap();
    assert!(again["jobs"].as_array().unwrap().is_empty());
}

#[test]
fn a_report_is_accepted_for_a_taken_job_and_only_once() {
    let r = rigged(Some("sidecar"), true, true);
    r.link.queue().submit(reply_job("a")).unwrap();
    let report = json!({"id": "a", "result": "delivered", "session": ID});
    let early = r
        .ops
        .call(&sidecar(), ops::AGY_SIDECAR_REPORT, &report)
        .unwrap();
    assert_eq!(early["accepted"], false);
    r.ops
        .call(&sidecar(), ops::AGY_SIDECAR_TAKE, &json!({"waitMs": 10}))
        .unwrap();
    let first = r
        .ops
        .call(&sidecar(), ops::AGY_SIDECAR_REPORT, &report)
        .unwrap();
    assert_eq!(first["accepted"], true);
    let second = r
        .ops
        .call(&sidecar(), ops::AGY_SIDECAR_REPORT, &report)
        .unwrap();
    assert_eq!(second["accepted"], false);
    let (kind, _) = code(r.ops.call(
        &sidecar(),
        ops::AGY_SIDECAR_REPORT,
        &json!({"id": "a", "result": "maybe"}),
    ));
    assert_eq!(kind, ErrorCode::BadRequest);
}

#[test]
fn a_taken_job_is_not_lost_when_the_sidecar_is_gone_and_the_next_take_gets_it() {
    let r = rigged(Some("sidecar"), true, true);
    r.link.queue().submit(reply_job("a")).unwrap();
    let ghost = from(Some(GONE_PID), ClientKind::Hook);
    let none = r
        .ops
        .call(&ghost, ops::AGY_SIDECAR_TAKE, &json!({"waitMs": 400}))
        .unwrap();
    assert!(none["jobs"].as_array().unwrap().is_empty());
    assert_eq!(r.link.queue().waiting(), 1);
    let out = r
        .ops
        .call(&sidecar(), ops::AGY_SIDECAR_TAKE, &json!({"waitMs": 10}))
        .unwrap();
    assert_eq!(out["jobs"].as_array().unwrap().len(), 1);
}

#[test]
fn a_report_counts_as_the_sidecar_being_there_so_the_gap_before_its_next_take_is_not_down() {
    let r = rigged(Some("sidecar"), true, true);
    assert!(!r.link.running());
    r.ops
        .call(
            &sidecar(),
            ops::AGY_SIDECAR_REPORT,
            &json!({"id": "nothing", "result": "failed", "error": "x"}),
        )
        .unwrap();
    assert!(r.link.running());
}

#[test]
fn an_older_take_that_wakes_after_a_newer_one_does_not_put_its_process_back() {
    let r = rigged(Some("sidecar"), true, true);
    let old = {
        let ops = r.ops.clone();
        std::thread::spawn(move || {
            ops.call(&sidecar(), ops::AGY_SIDECAR_TAKE, &json!({"waitMs": 600}))
                .unwrap()
        })
    };
    std::thread::sleep(Duration::from_millis(100));
    let newer = from(Some(NEWER_PID), ClientKind::Hook);
    r.ops
        .call(&newer, ops::AGY_SIDECAR_TAKE, &json!({"waitMs": 100}))
        .unwrap();
    old.join().unwrap();
    assert_eq!(r.link.queue().last_pid(), Some(NEWER_PID));
}

#[test]
fn one_take_can_ask_for_just_one_job() {
    let r = rigged(Some("sidecar"), true, true);
    for id in ["a", "b"] {
        r.link.queue().submit(reply_job(id)).unwrap();
    }
    let out = r
        .ops
        .call(
            &sidecar(),
            ops::AGY_SIDECAR_TAKE,
            &json!({"waitMs": 10, "max": 1}),
        )
        .unwrap();
    assert_eq!(out["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(r.link.queue().waiting(), 1);
}

#[test]
fn a_report_must_name_a_real_conversation_id() {
    let r = rigged(Some("sidecar"), true, true);
    r.link.queue().submit(reply_job("a")).unwrap();
    r.ops
        .call(&sidecar(), ops::AGY_SIDECAR_TAKE, &json!({"waitMs": 10}))
        .unwrap();
    for bad in ["../x", "agy:abc", "--help", ""] {
        for result in ["delivered", "unconfirmed"] {
            let (kind, _) = code(r.ops.call(
                &sidecar(),
                ops::AGY_SIDECAR_REPORT,
                &json!({"id": "a", "result": result, "session": bad}),
            ));
            assert_eq!(kind, ErrorCode::BadRequest, "{result} {bad:?}");
        }
    }
    let ok = r
        .ops
        .call(
            &sidecar(),
            ops::AGY_SIDECAR_REPORT,
            &json!({"id": "a", "result": "delivered", "session": ID}),
        )
        .unwrap();
    assert_eq!(ok["accepted"], true);
}

#[test]
fn a_sidecar_that_died_is_not_running_even_inside_the_grace_and_sends_fail_at_once() {
    let alive = Arc::new(AtomicBool::new(true));
    let flag = alive.clone();
    let link = SidecarLink::with(
        Box::new(now_ms),
        Patience {
            pickup: Duration::from_secs(5),
            run: Duration::from_secs(5),
        },
        Box::new(move |_| flag.load(Ordering::SeqCst)),
    );
    link.saw(SIDECAR_PID);
    assert!(link.running());
    alive.store(false, Ordering::SeqCst);
    assert!(!link.running());
    let began = Instant::now();
    assert_eq!(
        link.reply("row-1", ID, "hello"),
        Err(Delivery::Failed(NOT_RUNNING.into()))
    );
    assert!(began.elapsed() < Duration::from_secs(1));
    assert_eq!(link.queue().waiting(), 0);
}

#[test]
fn a_message_waiting_for_a_sidecar_that_dies_is_withdrawn_at_once_with_the_plain_words() {
    let alive = Arc::new(AtomicBool::new(true));
    let flag = alive.clone();
    let link = SidecarLink::with(
        Box::new(now_ms),
        Patience {
            pickup: Duration::from_secs(30),
            run: Duration::from_secs(30),
        },
        Box::new(move |_| flag.load(Ordering::SeqCst)),
    );
    link.saw(SIDECAR_PID);
    let killer = {
        let alive = alive.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            alive.store(false, Ordering::SeqCst);
        })
    };
    let began = Instant::now();
    assert_eq!(
        link.reply("row-1", ID, "hello"),
        Err(Delivery::Failed(NOT_RUNNING.into()))
    );
    assert!(began.elapsed() < Duration::from_secs(5));
    assert_eq!(link.queue().waiting(), 0);
    killer.join().unwrap();
}

#[test]
fn the_same_attempt_reported_unsure_is_never_sent_twice() {
    let r = rigged(Some("sidecar"), true, true);
    r.link.queue().seen(now_ms());
    let first = answer_with(&r, Outcome::Unconfirmed { session: None });
    assert_eq!(r.link.reply("row-1-1", ID, "hello"), Err(Delivery::Cut));
    first.join().unwrap();
    assert_eq!(r.link.reply("row-1-1", ID, "hello"), Err(Delivery::Cut));
    assert_eq!(r.link.queue().waiting(), 0);
    let second = answer_with(&r, Outcome::Delivered { session: None });
    assert_eq!(r.link.reply("row-1-2", ID, "hello"), Ok(()));
    second.join().unwrap();
}

#[test]
fn a_new_message_for_a_conversation_drops_the_stop_asked_before_it() {
    let r = rigged(Some("sidecar"), true, true);
    r.link.queue().seen(now_ms());
    r.link.stop(ID).unwrap();
    let taker = answer_with(&r, Outcome::Delivered { session: None });
    assert_eq!(r.link.reply("row-1-1", ID, "again"), Ok(()));
    taker.join().unwrap();
    let out = r
        .ops
        .call(
            &sidecar(),
            ops::AGY_HOOK,
            &json!({"event": "pre-invocation", "input": json!({"conversationId": ID}).to_string()}),
        )
        .unwrap();
    assert_eq!(out, json!({"output": null}));
}

fn answer_with(r: &Rigged, outcome: Outcome) -> std::thread::JoinHandle<()> {
    let queue = r.link.clone();
    std::thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(job) = queue.queue().take(Duration::from_millis(20)).pop() {
                queue.queue().report(&job.id, outcome.clone());
                return;
            }
            assert!(Instant::now() < end);
        }
    })
}

#[test]
fn a_reply_waits_for_the_sidecar_and_maps_what_it_reports() {
    let cases = [
        (Outcome::Delivered { session: None }, Ok(())),
        (Outcome::Unconfirmed { session: None }, Err(Delivery::Cut)),
        (
            Outcome::Failed("no such thing".into()),
            Err(Delivery::Failed("no such thing".into())),
        ),
    ];
    for (outcome, want) in cases {
        let r = rigged(Some("sidecar"), true, true);
        r.link.queue().seen(now_ms());
        let sidecar = answer_with(&r, outcome);
        assert_eq!(r.link.reply("row-1", ID, "hello"), want);
        sidecar.join().unwrap();
    }
}

#[test]
fn a_reply_with_no_sidecar_running_fails_visibly_and_queues_nothing() {
    let r = rigged(Some("sidecar"), true, true);
    assert_eq!(
        r.link.reply("row-1", ID, "hello"),
        Err(Delivery::Failed(NOT_RUNNING.into()))
    );
    assert_eq!(r.link.queue().waiting(), 0);
}

#[test]
fn a_message_nobody_picks_up_is_withdrawn_and_one_taken_but_unanswered_is_unsure() {
    let r = rigged(Some("sidecar"), true, true);
    r.link.queue().seen(now_ms());
    assert_eq!(
        r.link.reply("row-1", ID, "hello"),
        Err(Delivery::Failed(NOT_PICKED_UP.into()))
    );
    assert_eq!(r.link.queue().waiting(), 0);
    let taker = {
        let link = r.link.clone();
        std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(5);
            while link.queue().take(Duration::from_millis(20)).is_empty() {
                assert!(Instant::now() < end);
            }
        })
    };
    assert_eq!(r.link.reply("row-2", ID, "hello"), Err(Delivery::Cut));
    taker.join().unwrap();
}

#[test]
fn a_reply_to_something_that_is_not_a_conversation_id_is_refused_before_queueing() {
    let r = rigged(Some("sidecar"), true, true);
    r.link.queue().seen(now_ms());
    for bad in ["", "../x", "agy:abc", "--help"] {
        assert!(matches!(
            r.link.reply("row", bad, "hi"),
            Err(Delivery::Failed(_))
        ));
    }
    assert_eq!(r.link.queue().waiting(), 0);
}

fn event(tool: &str, command: &str) -> String {
    json!({
        "conversationId": ID,
        "cwd": "/w",
        "hookEventName": "PreToolUse",
        "toolCall": {"id": "t1", "name": tool, "args": {"CommandLine": command}},
    })
    .to_string()
}

fn pre_tool(input: String, wait_ms: u64) -> Value {
    json!({"event": "pre-tool", "input": input, "waitMs": wait_ms})
}

fn asked(r: &Rigged, args: Value) -> (std::thread::JoinHandle<Value>, String) {
    let ops = r.ops.clone();
    let before = r.room.named("prompt.pending").len();
    let thread = std::thread::spawn(move || ops.call(&sidecar(), ops::AGY_HOOK, &args).unwrap());
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(p) = r.room.named("prompt.pending").get(before) {
            return (thread, p["id"].as_str().unwrap().to_string());
        }
        assert!(Instant::now() < end, "never pending");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_tool_the_user_allows_in_notchling_is_auto_approved() {
    let r = rigged(Some("sidecar"), true, true);
    let (thread, id) = asked(&r, pre_tool(event("run_command", "npm test"), 10_000));
    let pending = r.room.named("prompt.pending");
    assert_eq!(pending[0]["sessionId"], format!("agy:{ID}"));
    assert_eq!(pending[0]["toolName"], "run_command");
    assert_eq!(pending[0]["input"]["command"], "npm test");
    r.acting
        .call(
            &app(),
            ops::SESSION_PERMISSION,
            &json!({"id": id, "decision": "allow"}),
        )
        .unwrap();
    assert_eq!(
        thread.join().unwrap(),
        json!({"output": {"decision": "auto_approve"}})
    );
}

#[test]
fn a_tool_the_user_denies_is_denied_with_a_reason() {
    let r = rigged(Some("sidecar"), true, true);
    let (thread, id) = asked(&r, pre_tool(event("run_command", "rm -rf x"), 10_000));
    r.acting
        .call(
            &app(),
            ops::SESSION_PERMISSION,
            &json!({"id": id, "decision": "deny"}),
        )
        .unwrap();
    let out = thread.join().unwrap();
    assert_eq!(out["output"]["decision"], "deny");
    assert!(out["output"]["reason"].is_string());
}

#[test]
fn nobody_answering_in_time_leaves_antigravity_to_decide() {
    let r = rigged(Some("sidecar"), true, true);
    let out = r
        .ops
        .call(
            &sidecar(),
            ops::AGY_HOOK,
            &pre_tool(event("run_command", "ls"), 60),
        )
        .unwrap();
    assert_eq!(out, json!({"output": null}));
    assert!(r.acting.prompts().list().is_empty());
}

#[test]
fn the_hooks_do_nothing_unless_the_sidecar_channel_is_chosen() {
    for channel in [None, Some("acp"), Some("app"), Some("remote")] {
        let r = rigged(channel, true, true);
        r.link.stops().ask(ID);
        let tool = r
            .ops
            .call(
                &sidecar(),
                ops::AGY_HOOK,
                &pre_tool(event("run_command", "ls"), 50),
            )
            .unwrap();
        assert_eq!(tool, json!({"output": null}), "{channel:?}");
        assert!(r.room.named("prompt.pending").is_empty());
        let stop = r
            .ops
            .call(
                &sidecar(),
                ops::AGY_HOOK,
                &json!({"event": "pre-invocation", "input": json!({"conversationId": ID}).to_string()}),
            )
            .unwrap();
        assert_eq!(stop, json!({"output": null}), "{channel:?}");
    }
}

#[test]
fn a_stop_ends_the_turn_once_at_the_next_model_call_and_only_for_that_conversation() {
    let r = rigged(Some("sidecar"), true, true);
    let invocation = |id: &str| {
        r.ops
            .call(
                &sidecar(),
                ops::AGY_HOOK,
                &json!({"event": "pre-invocation", "input": json!({"conversationId": id}).to_string()}),
            )
            .unwrap()
    };
    assert_eq!(invocation(ID), json!({"output": null}));
    r.link.stop(ID).unwrap();
    assert_eq!(
        invocation("5ce2bd47-0c1e-4e6b-8f43-6aa0e2f1d3aa"),
        json!({"output": null})
    );
    let out = invocation(ID);
    assert_eq!(out["output"]["terminationBehavior"], "terminate");
    assert_eq!(invocation(ID), json!({"output": null}));
    assert!(r.link.stop("../x").is_err());
}

#[test]
fn garbage_input_and_unknown_events_print_nothing() {
    let r = rigged(Some("sidecar"), true, true);
    for input in ["not json", "{}", "{\"conversationId\": \"../x\"}"] {
        for event in ["pre-tool", "pre-invocation"] {
            let out = r
                .ops
                .call(
                    &sidecar(),
                    ops::AGY_HOOK,
                    &json!({"event": event, "input": input}),
                )
                .unwrap();
            assert_eq!(out, json!({"output": null}), "{event} {input}");
        }
    }
    let (kind, _) = code(r.ops.call(
        &sidecar(),
        ops::AGY_HOOK,
        &json!({"event": "post-tool", "input": "{}"}),
    ));
    assert_eq!(kind, ErrorCode::BadRequest);
}

#[path = "agy_sidecar_setup_tests.rs"]
mod setup_tests;
