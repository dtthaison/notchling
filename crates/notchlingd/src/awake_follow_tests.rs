use crate::acting_ops_tests::FakeRoom;
use crate::awake_loop::*;
use crate::daemon::FirstScan;
use crate::daemon_wiring::Wired;
use crate::layout::Layout;
use crate::limits_ops_tests::TempDir;
use notchling_engine::awake::{FakePower, Kind, Mode, Outcome, Setting, Work};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::settings::Settings;
use notchling_local_api::{ops, Caller, ClientKind, Handler, OpSpec, Tier};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn working() -> Agent {
    Agent {
        id: "a".into(),
        name: "n".into(),
        project: "p".into(),
        cwd: "/tmp".into(),
        group: AgentGroup::Working,
        label: "x".into(),
        detail: None,
        since_ms: 0,
        target: AgentTarget::Job("job-7".into()),
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
    awake: AwakeLoop,
    power: FakePower,
    mode: Arc<Mutex<Setting>>,
    app: Arc<AtomicBool>,
    working: Arc<AtomicBool>,
    said: Arc<Mutex<Vec<Option<Kind>>>>,
}

fn rig() -> Rig {
    let power = FakePower::new();
    let mode = Arc::new(Mutex::new(Setting {
        mode: Mode::On,
        display: false,
    }));
    let app = Arc::new(AtomicBool::new(true));
    let busy = Arc::new(AtomicBool::new(false));
    let said = Arc::new(Mutex::new(Vec::new()));
    let (m, b, a, s) = (mode.clone(), busy.clone(), app.clone(), said.clone());
    let awake = AwakeLoop::new(
        Sources {
            setting: Box::new(move || Some(*m.lock().unwrap())),
            agents: Box::new(move || {
                if b.load(Ordering::SeqCst) {
                    vec![working()]
                } else {
                    Vec::new()
                }
            }),
            work: Box::new(Work::default),
        },
        Box::new(power.clone()),
    )
    .with_attached(Box::new(move || a.load(Ordering::SeqCst)))
    .with_announce(Box::new(move |holding| s.lock().unwrap().push(holding)));
    Rig {
        awake,
        power,
        mode,
        app,
        working: busy,
        said,
    }
}

#[test]
fn quitting_the_app_lets_go_and_the_next_attach_holds_again() {
    let r = rig();
    assert_eq!(r.awake.tick_once(), Outcome::Held(Kind::System));
    r.app.store(false, Ordering::SeqCst);
    assert_eq!(r.awake.tick_once(), Outcome::Released);
    assert_eq!(r.awake.holding(), None);
    assert_eq!(r.awake.tick_once(), Outcome::Unchanged);
    r.app.store(true, Ordering::SeqCst);
    assert_eq!(r.awake.tick_once(), Outcome::Held(Kind::System));
    assert_eq!(
        r.power.events(),
        ["hold system", "release system", "hold system"]
    );
}

#[test]
fn without_an_app_nothing_is_taken_whatever_the_mode() {
    let r = rig();
    r.app.store(false, Ordering::SeqCst);
    assert_eq!(r.awake.tick_once(), Outcome::Unchanged);
    r.mode.lock().unwrap().mode = Mode::WhileWorking;
    r.working.store(true, Ordering::SeqCst);
    assert_eq!(r.awake.tick_once(), Outcome::Unchanged);
    r.mode.lock().unwrap().display = true;
    assert_eq!(r.awake.tick_once(), Outcome::Unchanged);
    assert!(r.power.events().is_empty());
    r.app.store(true, Ordering::SeqCst);
    assert_eq!(r.awake.tick_once(), Outcome::Held(Kind::Display));
}

#[test]
fn a_refusal_while_attached_is_tried_again_after_a_quit_and_a_return() {
    let r = rig();
    r.power.refuse_with(Some("no"));
    assert_eq!(r.awake.tick_once(), Outcome::Failed("no".into()));
    assert_eq!(r.awake.tick_once(), Outcome::Unchanged);
    r.app.store(false, Ordering::SeqCst);
    r.awake.tick_once();
    r.app.store(true, Ordering::SeqCst);
    assert_eq!(
        r.awake.tick_once(),
        Outcome::Failed("no".into()),
        "the same refusal is worth saying again for a new run of the app"
    );
}

#[test]
fn the_hold_is_announced_when_it_changes_and_never_when_it_doesnt() {
    let r = rig();
    r.awake.tick_once();
    r.awake.tick_once();
    r.mode.lock().unwrap().display = true;
    r.awake.tick_once();
    r.awake.tick_once();
    r.app.store(false, Ordering::SeqCst);
    r.awake.tick_once();
    r.awake.tick_once();
    assert_eq!(
        *r.said.lock().unwrap(),
        [Some(Kind::System), Some(Kind::Display), None]
    );
}

#[test]
fn a_hold_the_system_refused_is_not_announced() {
    let r = rig();
    r.power.refuse_with(Some("no"));
    r.awake.tick_once();
    assert!(r.said.lock().unwrap().is_empty());
}

#[test]
fn turning_the_mode_off_is_announced_as_nothing_held() {
    let r = rig();
    r.awake.tick_once();
    r.mode.lock().unwrap().mode = Mode::Off;
    r.awake.tick_once();
    assert_eq!(*r.said.lock().unwrap(), [Some(Kind::System), None]);
}

struct Daemon {
    _dir: TempDir,
    layout: Layout,
    room: Arc<FakeRoom>,
    power: FakePower,
    wired: Wired,
}

fn daemon(tag: &str) -> Daemon {
    let dir = TempDir::new(tag);
    let layout = Layout::resolve(None, Some(dir.root().as_os_str()), || None).unwrap();
    Settings::open(&layout.data_dir.join("settings.json"))
        .unwrap()
        .set("mode", json!("on"))
        .unwrap();
    let room = Arc::new(FakeRoom::default());
    let power = FakePower::new();
    let wired = Wired::build_with_power(
        &layout,
        Arc::new(Mutex::new(json!([]))),
        room.clone(),
        (0, 0),
        Box::new(power.clone()),
    );
    Daemon {
        _dir: dir,
        layout,
        room,
        power,
        wired,
    }
}

fn app_caller() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

#[test]
fn the_daemon_holds_only_while_an_app_is_attached_and_says_so_to_the_room() {
    let d = daemon("awake-follow-room");
    assert_eq!(d.wired.awake.tick_once(), Outcome::Unchanged, "no app yet");
    d.room.app.store(true, Ordering::SeqCst);
    assert_eq!(d.wired.awake.tick_once(), Outcome::Held(Kind::System));
    d.room.app.store(false, Ordering::SeqCst);
    assert_eq!(d.wired.awake.tick_once(), Outcome::Released);
    assert_eq!(
        d.room.named("awake.changed"),
        [json!({"holding": "system"}), json!({"holding": null})]
    );
    assert_eq!(d.power.events(), ["hold system", "release system"]);
    d.wired.close();
}

#[test]
fn awake_state_answers_what_is_held_now_to_the_app_and_is_a_quiet_read() {
    let d = daemon("awake-follow-op");
    let ops = d.wired.ops(
        &d.layout,
        Arc::new(Mutex::new(json!([]))),
        FirstScan::open(),
        Duration::ZERO,
    );
    assert_eq!(ops.spec(ops::AWAKE_STATE), Some(OpSpec::APP_QUIET));
    let ask = || {
        ops.call(&app_caller(), ops::AWAKE_STATE, &json!({}))
            .unwrap()
    };
    assert_eq!(ask(), json!({"holding": null}));
    d.room.app.store(true, Ordering::SeqCst);
    d.wired.awake.tick_once();
    assert_eq!(ask(), json!({"holding": "system"}));
    d.room.app.store(false, Ordering::SeqCst);
    d.wired.awake.tick_once();
    assert_eq!(ask(), json!({"holding": null}));
    d.wired.close();
}
