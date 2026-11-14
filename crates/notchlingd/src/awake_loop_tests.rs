use crate::awake_loop::*;
use crate::layout::Layout;
use crate::limits_ops_tests::TempDir;
use notchling_engine::awake::{FakePower, Kind, Mode, Outcome, Setting, Work};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::settings::Settings;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn agent(group: AgentGroup) -> Agent {
    Agent {
        id: "a".into(),
        name: "n".into(),
        project: "p".into(),
        cwd: "/tmp".into(),
        group,
        label: "x".into(),
        detail: None,
        since_ms: 0,
        target: AgentTarget::Process(7),
        suggested_reply: None,
        transcript_path: None,
        host: String::new(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
    }
}

struct Rig {
    awake: Arc<AwakeLoop>,
    power: FakePower,
    setting: Arc<Mutex<Option<Setting>>>,
    agents: Arc<Mutex<Vec<Agent>>>,
    codex: Arc<AtomicUsize>,
}

fn rig() -> Rig {
    let power = FakePower::new();
    let setting = Arc::new(Mutex::new(Some(Setting::default())));
    let agents = Arc::new(Mutex::new(Vec::new()));
    let codex = Arc::new(AtomicUsize::new(0));
    let (s, a, c) = (setting.clone(), agents.clone(), codex.clone());
    let awake = Arc::new(
        AwakeLoop::new(
            Sources {
                setting: Box::new(move || *s.lock().unwrap()),
                agents: Box::new(move || a.lock().unwrap().clone()),
                work: Box::new(move || Work {
                    codex_turns: c.load(Ordering::SeqCst),
                    agy_sessions: 0,
                }),
            },
            Box::new(power.clone()),
        )
        .with_tick(Duration::from_millis(20)),
    );
    Rig {
        awake,
        power,
        setting,
        agents,
        codex,
    }
}

impl Rig {
    fn set(&self, mode: Mode, display: bool) {
        *self.setting.lock().unwrap() = Some(Setting { mode, display });
    }
}

#[test]
fn a_working_session_holds_the_mac_awake_and_its_end_lets_go() {
    let r = rig();
    r.set(Mode::WhileWorking, false);
    assert_eq!(r.awake.tick_once(), Outcome::Unchanged);
    *r.agents.lock().unwrap() = vec![agent(AgentGroup::Working)];
    assert_eq!(r.awake.tick_once(), Outcome::Held(Kind::System));
    assert_eq!(r.awake.holding(), Some(Kind::System));
    *r.agents.lock().unwrap() = vec![agent(AgentGroup::Completed)];
    assert_eq!(r.awake.tick_once(), Outcome::Released);
    assert_eq!(r.power.events(), ["hold system", "release system"]);
}

#[test]
fn a_codex_turn_notchling_runs_holds_it_too() {
    let r = rig();
    r.set(Mode::WhileWorking, false);
    r.codex.store(1, Ordering::SeqCst);
    assert_eq!(r.awake.tick_once(), Outcome::Held(Kind::System));
    r.codex.store(0, Ordering::SeqCst);
    assert_eq!(r.awake.tick_once(), Outcome::Released);
}

#[test]
fn always_on_holds_with_nothing_running_and_the_display_flag_swaps_it() {
    let r = rig();
    r.set(Mode::On, false);
    assert_eq!(r.awake.tick_once(), Outcome::Held(Kind::System));
    assert_eq!(r.awake.tick_once(), Outcome::Unchanged);
    r.set(Mode::On, true);
    assert_eq!(r.awake.tick_once(), Outcome::Held(Kind::Display));
    r.set(Mode::Off, true);
    assert_eq!(r.awake.tick_once(), Outcome::Released);
    assert_eq!(
        r.power.events(),
        [
            "hold system",
            "hold display",
            "release system",
            "release display"
        ]
    );
}

#[test]
fn a_setting_nobody_could_read_changes_nothing() {
    let r = rig();
    r.set(Mode::On, false);
    r.awake.tick_once();
    *r.setting.lock().unwrap() = None;
    assert_eq!(r.awake.tick_once(), Outcome::Unchanged);
    assert_eq!(r.awake.holding(), Some(Kind::System));
}

#[test]
fn closing_lets_go_of_the_hold() {
    let r = rig();
    r.set(Mode::On, false);
    r.awake.tick_once();
    r.awake.close();
    assert_eq!(r.awake.holding(), None);
    assert_eq!(r.power.events(), ["hold system", "release system"]);
}

#[test]
fn the_loop_holds_at_once_and_follows_a_change_within_a_tick() {
    let r = rig();
    r.set(Mode::On, false);
    let awake = r.awake.clone();
    awake.spawn_loop();
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while awake.holding().is_none() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(awake.holding(), Some(Kind::System));
    r.set(Mode::Off, false);
    while awake.holding().is_some() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(awake.holding(), None);
    awake.close();
}

#[test]
fn only_a_real_root_holds_a_real_power_assertion() {
    let dir = TempDir::new("awake-fixture");
    let fixture = Layout::resolve(None, Some(dir.root().as_os_str()), || None).unwrap();
    assert!(fixture.fixture);
    assert!(!holds_for_real(&fixture));
    let real = Layout::resolve(None, None, || Some((dir.path("home"), dir.path("data")))).unwrap();
    assert!(!real.fixture);
    assert!(holds_for_real(&real));
}

#[test]
fn the_store_reader_gives_what_the_settings_say() {
    let dir = TempDir::new("awake-store");
    let file = dir.path("settings.json");
    let read = store_setting(file.clone());
    assert_eq!(read(), Some(Setting::default()));
    let mut store = Settings::open(&file).unwrap();
    store.set("mode", json!("whileWorking")).unwrap();
    store.set("keepDisplayOn", json!(true)).unwrap();
    assert_eq!(
        read(),
        Some(Setting {
            mode: Mode::WhileWorking,
            display: true
        })
    );
}

#[test]
fn the_store_reader_says_nothing_when_the_store_cannot_be_read() {
    let dir = TempDir::new("awake-bad-store");
    dir.write("notchling.db", "this is not a database, it is a sentence");
    assert_eq!(store_setting(dir.path("settings.json"))(), None);
}

#[test]
fn the_session_list_reader_reads_the_daemons_cached_list() {
    let list = Arc::new(Mutex::new(json!([])));
    let read = agents_in(list.clone());
    assert!(read().is_empty());
    *list.lock().unwrap() = serde_json::to_value(vec![
        agent(AgentGroup::Working),
        agent(AgentGroup::Completed),
    ])
    .unwrap();
    let agents = read();
    assert_eq!(agents.len(), 2);
    assert_eq!(agents[0].group, AgentGroup::Working);
    *list.lock().unwrap() = Value::String("not a list".into());
    assert!(read().is_empty());
}

#[test]
fn the_stored_mode_and_the_daemons_session_list_drive_the_hold() {
    let dir = TempDir::new("awake-real-readers");
    let file = dir.path("settings.json");
    let list = Arc::new(Mutex::new(json!([])));
    let power = FakePower::new();
    let awake = AwakeLoop::new(
        Sources {
            setting: Box::new(store_setting(file.clone())),
            agents: Box::new(agents_in(list.clone())),
            work: Box::new(Work::default),
        },
        Box::new(power.clone()),
    );
    assert_eq!(awake.tick_once(), Outcome::Unchanged);
    Settings::open(&file)
        .unwrap()
        .set("mode", json!("whileWorking"))
        .unwrap();
    assert_eq!(awake.tick_once(), Outcome::Unchanged);
    *list.lock().unwrap() = serde_json::to_value(vec![agent(AgentGroup::Working)]).unwrap();
    assert_eq!(awake.tick_once(), Outcome::Held(Kind::System));
    Settings::open(&file)
        .unwrap()
        .set("keepDisplayOn", json!(true))
        .unwrap();
    assert_eq!(awake.tick_once(), Outcome::Held(Kind::Display));
    Settings::open(&file)
        .unwrap()
        .set("mode", json!("off"))
        .unwrap();
    assert_eq!(awake.tick_once(), Outcome::Released);
    assert_eq!(
        power.events(),
        [
            "hold system",
            "hold display",
            "release system",
            "release display"
        ]
    );
}

#[test]
fn the_daemon_starts_the_loop_with_its_own_settings_and_lets_go_when_it_closes() {
    let dir = TempDir::new("awake-wired");
    let layout = Layout::resolve(None, Some(dir.root().as_os_str()), || None).unwrap();
    Settings::open(&layout.data_dir.join("settings.json"))
        .unwrap()
        .set("mode", json!("on"))
        .unwrap();
    let power = FakePower::new();
    let wired = crate::daemon_wiring::Wired::build_with_power(
        &layout,
        Arc::new(Mutex::new(json!([]))),
        Arc::new(crate::acting_ops_tests::FakeRoom::default()),
        (0, 0),
        Box::new(power.clone()),
    );
    assert_eq!(wired.awake.holding(), None, "nothing is held before start");
    wired.start();
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while wired.awake.holding().is_none() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(wired.awake.holding(), Some(Kind::System));
    wired.close();
    assert_eq!(wired.awake.holding(), None);
    assert_eq!(power.events(), ["hold system", "release system"]);
}
