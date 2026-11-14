use crate::acting_ops_tests::{row, setup, FakeRoom, PROMPT};
use crate::layout::Layout;
use crate::limits_ops_tests::TempDir;
use crate::wake_loop::*;
use notchling_engine::control::wake::{PROMPT as NUDGE, SETTLE_MS, WINDOW_MS};
use notchling_engine::control::Nudge;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::settings::Settings;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn job(id: &str, group: AgentGroup) -> Agent {
    Agent {
        id: id.into(),
        name: "n".into(),
        project: "p".into(),
        cwd: "/tmp".into(),
        group,
        label: "x".into(),
        detail: None,
        since_ms: 0,
        target: AgentTarget::Job(format!("job-{id}")),
        suggested_reply: None,
        transcript_path: None,
        host: String::new(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
    }
}

struct FakeWakes(Arc<AtomicUsize>);

impl Wakes for FakeWakes {
    fn woke(&mut self) -> bool {
        self.0.swap(0, Ordering::SeqCst) > 0
    }
}

struct Rig {
    wake: Arc<WakeLoop>,
    woke: Arc<AtomicUsize>,
    now: Arc<AtomicI64>,
    enabled: Arc<AtomicBool>,
    can_act: Arc<AtomicBool>,
    agents: Arc<Mutex<Vec<Agent>>>,
    cuts: Arc<Mutex<HashMap<String, String>>>,
    sent: Arc<Mutex<Vec<(String, String)>>>,
    refuse: Arc<AtomicBool>,
    reads: Arc<AtomicUsize>,
}

fn rig() -> Rig {
    let woke = Arc::new(AtomicUsize::new(0));
    let now = Arc::new(AtomicI64::new(0));
    let enabled = Arc::new(AtomicBool::new(true));
    let can_act = Arc::new(AtomicBool::new(true));
    let agents = Arc::new(Mutex::new(Vec::new()));
    let cuts = Arc::new(Mutex::new(HashMap::new()));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let refuse = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(AtomicUsize::new(0));
    let (e, c, a, k, s, r, n, rd) = (
        enabled.clone(),
        can_act.clone(),
        agents.clone(),
        cuts.clone(),
        sent.clone(),
        refuse.clone(),
        now.clone(),
        reads.clone(),
    );
    let wake = Arc::new(
        WakeLoop::new(
            Sources {
                enabled: Box::new(move || e.load(Ordering::SeqCst)),
                can_act: Box::new(move || c.load(Ordering::SeqCst)),
                agents: Box::new(move || {
                    rd.fetch_add(1, Ordering::SeqCst);
                    a.lock().unwrap().clone()
                }),
                cut_off: Box::new(move |agent| k.lock().unwrap().get(&agent.id).cloned()),
                now_ms: Box::new(move || n.load(Ordering::SeqCst)),
                nudge: Box::new(move |agent, text| {
                    s.lock().unwrap().push((agent.id.clone(), text.to_string()));
                    if r.load(Ordering::SeqCst) {
                        Err("the terminal wouldn't take it".into())
                    } else {
                        Ok(())
                    }
                }),
            },
            Box::new(FakeWakes(woke.clone())),
        )
        .with_tick(Duration::from_millis(10)),
    );
    Rig {
        wake,
        woke,
        now,
        enabled,
        can_act,
        agents,
        cuts,
        sent,
        refuse,
        reads,
    }
}

impl Rig {
    fn wake_at(&self, ms: i64) {
        self.now.store(ms, Ordering::SeqCst);
        self.woke.fetch_add(1, Ordering::SeqCst);
        self.wake.tick_once();
    }

    fn tick_at(&self, ms: i64) -> Vec<Nudge> {
        self.now.store(ms, Ordering::SeqCst);
        self.wake.tick_once()
    }

    fn cut(&self, id: &str, cut: &str) {
        self.cuts.lock().unwrap().insert(id.into(), cut.into());
    }
}

#[test]
fn a_cut_off_job_is_told_to_continue_once_after_the_settle_time() {
    let r = rig();
    *r.agents.lock().unwrap() = vec![job("a", AgentGroup::Completed)];
    r.cut("a", "cut-1");
    r.wake_at(0);
    assert!(
        r.tick_at(SETTLE_MS - 1).is_empty(),
        "the network is coming back"
    );
    let nudges = r.tick_at(SETTLE_MS);
    assert_eq!(nudges.len(), 1);
    assert_eq!(nudges[0].agent_id, "a");
    assert_eq!(nudges[0].job_id, "job-a");
    assert_eq!(
        *r.sent.lock().unwrap(),
        [("a".to_string(), NUDGE.to_string())]
    );
    assert!(
        r.tick_at(SETTLE_MS + 5_000).is_empty(),
        "one nudge per cut-off"
    );
    r.cut("a", "cut-2");
    assert_eq!(
        r.tick_at(SETTLE_MS + 6_000).len(),
        1,
        "a new cut-off is a new one"
    );
    assert_eq!(r.sent.lock().unwrap().len(), 2);
}

#[test]
fn nothing_is_sent_while_the_setting_is_off_or_the_daemon_may_not_act_and_it_waits_for_both() {
    let r = rig();
    *r.agents.lock().unwrap() = vec![job("a", AgentGroup::Completed)];
    r.cut("a", "cut-1");
    r.wake_at(0);
    r.enabled.store(false, Ordering::SeqCst);
    assert!(r.tick_at(SETTLE_MS).is_empty());
    r.enabled.store(true, Ordering::SeqCst);
    r.can_act.store(false, Ordering::SeqCst);
    assert!(r.tick_at(SETTLE_MS + 1_000).is_empty());
    assert!(r.sent.lock().unwrap().is_empty());
    r.can_act.store(true, Ordering::SeqCst);
    assert_eq!(
        r.tick_at(SETTLE_MS + 2_000).len(),
        1,
        "still inside the window"
    );
}

#[test]
fn a_cut_off_found_after_the_window_was_not_this_sleeps() {
    let r = rig();
    *r.agents.lock().unwrap() = vec![job("a", AgentGroup::Completed)];
    r.cut("a", "cut-1");
    r.wake_at(0);
    r.can_act.store(false, Ordering::SeqCst);
    r.tick_at(SETTLE_MS);
    r.can_act.store(true, Ordering::SeqCst);
    assert!(r.tick_at(WINDOW_MS + 1).is_empty());
    assert!(r.sent.lock().unwrap().is_empty());
}

#[test]
fn working_rows_terminals_and_rows_without_a_cut_off_get_nothing() {
    let r = rig();
    let mut terminal = job("t", AgentGroup::Completed);
    terminal.target = AgentTarget::Resume {
        session_id: "s".into(),
        cwd: "/tmp".into(),
    };
    *r.agents.lock().unwrap() = vec![
        job("w", AgentGroup::Working),
        terminal,
        job("quiet", AgentGroup::Completed),
        job("ok", AgentGroup::NeedsInput),
    ];
    r.cut("w", "cut-w");
    r.cut("t", "cut-t");
    r.cut("ok", "cut-ok");
    r.wake_at(0);
    let nudges = r.tick_at(SETTLE_MS);
    assert_eq!(nudges.len(), 1);
    assert_eq!(nudges[0].agent_id, "ok");
}

#[test]
fn without_a_wake_the_session_list_is_never_read() {
    let r = rig();
    *r.agents.lock().unwrap() = vec![job("a", AgentGroup::Completed)];
    r.cut("a", "cut-1");
    for ms in [0, 2_000, 4_000, SETTLE_MS * 3] {
        assert!(r.tick_at(ms).is_empty());
    }
    assert_eq!(r.reads.load(Ordering::SeqCst), 0);
    assert!(r.sent.lock().unwrap().is_empty());
}

#[test]
fn a_nudge_that_failed_is_not_sent_again_for_the_same_cut_off() {
    let r = rig();
    *r.agents.lock().unwrap() = vec![job("a", AgentGroup::Completed)];
    r.cut("a", "cut-1");
    r.refuse.store(true, Ordering::SeqCst);
    r.wake_at(0);
    assert_eq!(r.tick_at(SETTLE_MS).len(), 1);
    assert!(r.tick_at(SETTLE_MS + 2_000).is_empty());
    r.refuse.store(false, Ordering::SeqCst);
    r.cut("a", "cut-2");
    assert_eq!(
        r.tick_at(SETTLE_MS + 4_000).len(),
        1,
        "the row is free again"
    );
    assert_eq!(r.sent.lock().unwrap().len(), 2);
}

#[test]
fn a_second_wake_opens_a_new_window() {
    let r = rig();
    *r.agents.lock().unwrap() = vec![job("a", AgentGroup::Completed)];
    r.cut("a", "cut-1");
    r.wake_at(0);
    assert_eq!(r.tick_at(SETTLE_MS).len(), 1);
    let later = WINDOW_MS * 2;
    r.cut("a", "cut-2");
    r.wake_at(later);
    assert!(r.tick_at(later + SETTLE_MS - 1).is_empty());
    assert_eq!(r.tick_at(later + SETTLE_MS).len(), 1);
}

#[test]
fn the_loop_thread_follows_a_wake_and_closing_stops_it() {
    let r = rig();
    *r.agents.lock().unwrap() = vec![job("a", AgentGroup::Completed)];
    r.cut("a", "cut-1");
    r.wake.spawn_loop();
    r.woke.fetch_add(1, Ordering::SeqCst);
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while r.reads.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    r.now.store(SETTLE_MS, Ordering::SeqCst);
    while r.sent.lock().unwrap().is_empty() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(r.sent.lock().unwrap().len(), 1);
    r.wake.close();
    let seen = r.reads.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(
        r.reads.load(Ordering::SeqCst),
        seen,
        "the thread has stopped"
    );
}

#[test]
fn the_stored_choice_is_read_each_time_and_defaults_to_on() {
    let dir = TempDir::new("wake-store");
    let file = dir.path("settings.json");
    let read = store_enabled(file.clone());
    assert!(read(), "nothing stored: the Swift app's default");
    let mut store = Settings::open(&file).unwrap();
    store.set("continueAfterSleep", json!(false)).unwrap();
    assert!(!read());
    store.set("continueAfterSleep", json!(true)).unwrap();
    assert!(read());
}

#[test]
fn a_store_nobody_can_read_means_no_nudge() {
    let dir = TempDir::new("wake-bad-store");
    dir.write("notchling.db", "this is not a database, it is a sentence");
    assert!(!store_enabled(dir.path("settings.json"))());
}

#[test]
fn only_a_real_root_listens_for_the_mac_waking() {
    let dir = TempDir::new("wake-fixture");
    let fixture = Layout::resolve(None, Some(dir.root().as_os_str()), || None).unwrap();
    assert!(fixture.fixture);
    assert!(!listens_for_real(&fixture));
    let real = Layout::resolve(None, None, || Some((dir.path("home"), dir.path("data")))).unwrap();
    assert!(listens_for_real(&real));
    assert!(!wakes_for(&fixture).woke());
}

const CUT: &str = r#"{"type":"assistant","uuid":"u-cut","isApiErrorMessage":true,"streamCutCause":"stream_suspended","message":{"content":[{"type":"text","text":"API Error: Your computer went to sleep mid-response"}]}}"#;

type Attaches = Arc<Mutex<Vec<(String, Option<String>)>>>;

struct Glue {
    wake: WakeLoop,
    woke: Arc<AtomicUsize>,
    now: Arc<AtomicI64>,
    attached: Attaches,
    writes: Arc<Mutex<Vec<String>>>,
    acting: Arc<crate::acting::Acting>,
    room: Arc<FakeRoom>,
    _dir: TempDir,
}

fn glue(tag: &str, on: bool) -> Glue {
    let dir = TempDir::new(tag);
    let layout = Layout::resolve(None, Some(dir.root().as_os_str()), || None).unwrap();
    dir.write("t/cut.jsonl", &format!("{CUT}\n"));
    let transcript = dir.path("t/cut.jsonl");
    let list = json!([
        row(
            "job-a",
            json!({"job": "abcd1234"}),
            "completed",
            Some(&transcript.to_string_lossy())
        ),
        row("job-b", json!({"job": "ef567890"}), "completed", None),
        row("term", json!({"process": 4242}), "completed", None),
    ]);
    let s = setup(on, PROMPT, list.clone());
    let acting = s.ops.acting_switch();
    let (attached, writes, room) = (s.attached.clone(), s.writes.clone(), s.room.clone());
    let woke = Arc::new(AtomicUsize::new(0));
    let now = Arc::new(AtomicI64::new(0));
    let clock = now.clone();
    let wake = WakeLoop::for_daemon(
        &layout,
        Arc::new(Mutex::new(list)),
        Arc::new(s.ops),
        room.clone(),
        Box::new(FakeWakes(woke.clone())),
    )
    .with_clock(Box::new(move || clock.load(Ordering::SeqCst)));
    Glue {
        wake,
        woke,
        now,
        attached,
        writes,
        acting,
        room,
        _dir: dir,
    }
}

impl Glue {
    fn wake_and_settle(&self) -> Vec<Nudge> {
        self.now.store(0, Ordering::SeqCst);
        self.woke.fetch_add(1, Ordering::SeqCst);
        self.wake.tick_once();
        self.now.store(SETTLE_MS, Ordering::SeqCst);
        self.wake.tick_once()
    }

    fn typed(&self) -> String {
        self.writes.lock().unwrap().concat()
    }
}

#[test]
fn the_daemon_types_the_prompt_into_the_cut_off_job_and_only_that_one() {
    let g = glue("wake-glue", true);
    let nudges = g.wake_and_settle();
    assert_eq!(nudges.len(), 1, "{nudges:?}");
    assert_eq!(nudges[0].job_id, "abcd1234");
    assert_eq!(g.attached.lock().unwrap().len(), 1);
    assert_eq!(g.attached.lock().unwrap()[0].0, "abcd1234");
    assert!(g.typed().contains(NUDGE), "{}", g.typed());
}

#[test]
fn with_acting_off_nothing_is_attached_or_typed() {
    let g = glue("wake-glue-off", false);
    assert!(g.wake_and_settle().is_empty());
    assert!(g.attached.lock().unwrap().is_empty());
    assert!(g.typed().is_empty());
}

#[test]
fn with_no_app_attached_nothing_is_attached_or_typed_and_the_next_look_still_can() {
    let g = glue("wake-glue-noapp", true);
    g.room.app.store(false, Ordering::SeqCst);
    assert!(g.wake_and_settle().is_empty());
    assert!(g.typed().is_empty());
    g.room.app.store(true, Ordering::SeqCst);
    g.now.store(SETTLE_MS + 2_000, Ordering::SeqCst);
    assert_eq!(g.wake.tick_once().len(), 1);
}

#[test]
fn switching_acting_off_after_the_wake_stops_the_nudge() {
    let g = glue("wake-glue-switch", true);
    g.now.store(0, Ordering::SeqCst);
    g.woke.fetch_add(1, Ordering::SeqCst);
    g.wake.tick_once();
    g.acting.set(false);
    g.now.store(SETTLE_MS, Ordering::SeqCst);
    assert!(g.wake.tick_once().is_empty());
    assert!(g.typed().is_empty());
}

#[test]
fn the_setting_in_the_daemons_own_store_is_what_decides() {
    let g = glue("wake-glue-setting", true);
    let layout = Layout::resolve(None, Some(g._dir.root().as_os_str()), || None).unwrap();
    Settings::open(&layout.data_dir.join("settings.json"))
        .unwrap()
        .set("continueAfterSleep", json!(false))
        .unwrap();
    assert!(g.wake_and_settle().is_empty());
    assert!(g.typed().is_empty());
}

#[test]
fn the_daemon_starts_the_wake_loop_and_closing_it_stops_listening() {
    let dir = TempDir::new("wake-wired");
    let layout = Layout::resolve(None, Some(dir.root().as_os_str()), || None).unwrap();
    let woke = Arc::new(AtomicUsize::new(0));
    let wired = crate::daemon_wiring::Wired::build_with_hardware(
        &layout,
        Arc::new(Mutex::new(json!([]))),
        Arc::new(FakeRoom::default()),
        (0, 0),
        Box::new(notchling_engine::awake::FakePower::new()),
        Box::new(FakeWakes(woke.clone())),
    );
    wired.start();
    woke.fetch_add(1, Ordering::SeqCst);
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while woke.load(Ordering::SeqCst) != 0 && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(woke.load(Ordering::SeqCst), 0, "the loop heard the wake");
    wired.close();
    woke.fetch_add(1, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        woke.load(Ordering::SeqCst),
        1,
        "closed: nobody is listening"
    );
}
