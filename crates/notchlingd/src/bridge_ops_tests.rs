use super::*;
use crate::acting::{Acting, ACTING_OFF};
use crate::acting_ops::NoTerminal;
use crate::acting_ops_tests::{row, FakeRoom};
use crate::bridge_backend::{Approvals, DaemonActing};
use crate::inbox_ops::InboxOps;
use notchling_engine::bridge::live::Acting as _;
use notchling_engine::bridge::model::{Control, Session, SessionState};
use notchling_engine::sessions::Roots;
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

const DEVICES: &str = include_str!("../../engine/src/bridge/net/testdata/devices.swift.json");

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "notchlingd-bridge-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

fn swift_folder(data: &Path) {
    let bridge = data.join("bridge");
    std::fs::create_dir_all(&bridge).unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["Agent Console (test)".to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    let pfx = p12::PFX::new(
        cert.der(),
        &key.serialize_der(),
        None,
        "pw",
        "Agent Console",
    )
    .unwrap();
    std::fs::write(bridge.join("identity.p12"), pfx.to_der()).unwrap();
    std::fs::write(bridge.join("identity.pass"), "pw").unwrap();
    std::fs::write(bridge.join("cert.pem"), cert.pem()).unwrap();
    std::fs::write(bridge.join("devices.json"), DEVICES).unwrap();
    let access = json!({"enabled": true, "hostname": "h.example",
        "teamDomain": "t.cloudflareaccess.com", "audience": "a", "clientId": "i", "clientSecret": "s"});
    std::fs::write(bridge.join("access.json"), access.to_string()).unwrap();
}

fn free_ports() -> (u16, u16) {
    let a = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let b = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    (
        a.local_addr().unwrap().port(),
        b.local_addr().unwrap().port(),
    )
}

fn listening(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, u64, Option<SystemTime>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let meta = e.metadata().unwrap();
            if meta.is_dir() {
                stack.push(e.path());
            }
            out.push((e.path(), meta.len(), meta.modified().ok()));
        }
    }
    out.sort();
    out
}

fn acting_ops(on: bool, sessions: Arc<Mutex<Value>>, room: Arc<FakeRoom>) -> Arc<ActingOps> {
    Arc::new(ActingOps::new(
        Arc::new(Acting::in_memory(on)),
        sessions,
        Roots {
            main_dir: PathBuf::from("/nonexistent/claude"),
            lanes_file: PathBuf::from("/nonexistent/lanes.json"),
        },
        room,
        Box::new(NoTerminal),
    ))
}

fn bridge_ops(dir: &Path, lan: u16, tunnel: u16) -> (BridgeOps, Arc<FakeRoom>) {
    let room = Arc::new(FakeRoom::default());
    let sessions = Arc::new(Mutex::new(json!([])));
    let setup = BridgeSetup {
        data_dir: dir.join("AgentConsole"),
        home: dir.join("home"),
        images_dir: dir.join("phone-images"),
        projects_dir: dir.join("home/.claude/projects"),
        lan_port: lan,
        tunnel_port: tunnel,
        loopback_only: true,
        approval_wait: Duration::from_secs(5),
    };
    let acting = acting_ops(false, sessions.clone(), room.clone());
    let inbox = InboxOps::new(dir.join("data"), room.clone());
    (
        BridgeOps::new(
            setup,
            sessions,
            acting,
            room.clone(),
            inbox,
            Arc::new(EverySession::none()),
        ),
        room,
    )
}

#[test]
fn every_bridge_op_is_app_tier() {
    let dir = scratch("tiers");
    let (ops, _) = bridge_ops(&dir.0, 0, 0);
    for op in BRIDGE_OPS {
        assert_eq!(ops.spec(op), Some(OpSpec::APP), "{op}");
    }
    assert_eq!(ops.spec(ops::SESSIONS_LIST), None);
}

#[test]
fn off_at_the_start_binds_no_port_and_writes_nothing() {
    let dir = scratch("off");
    swift_folder(&dir.0.join("AgentConsole"));
    let before = snapshot(&dir.0);
    let (lan, tunnel) = free_ports();
    let (ops, _) = bridge_ops(&dir.0, lan, tunnel);
    assert!(!ops.is_on());
    assert_eq!(
        ops.call(ops::BRIDGE_STATUS, &json!({})).unwrap()["state"],
        "off"
    );
    let devices = ops.call(ops::BRIDGE_DEVICES, &json!({})).unwrap();
    assert_eq!(devices["devices"].as_array().unwrap().len(), 2);
    assert_eq!(devices["devices"][1]["name"], "Pixel 9 / work");
    let tunnel_status = ops.call(ops::BRIDGE_TUNNEL_STATUS, &json!({})).unwrap();
    assert_eq!(tunnel_status["enabled"], true);
    assert_eq!(tunnel_status["listening"], false);
    assert!(ops.call(ops::BRIDGE_PAIR_BEGIN, &json!({})).is_err());
    let revoke = json!({"device": "6D0F2A58-3B77-4C0E-9A41-5E8B1C7D2F90"});
    assert!(
        ops.call(ops::BRIDGE_REVOKE, &revoke).is_err(),
        "revoking writes"
    );
    assert!(
        !listening(lan) && !listening(tunnel),
        "nothing listens while off"
    );
    assert_eq!(snapshot(&dir.0), before, "nothing written while off");
}

#[test]
fn on_listens_on_both_ports_pairs_and_off_closes_them() {
    let dir = scratch("on");
    swift_folder(&dir.0.join("AgentConsole"));
    let (lan, tunnel) = free_ports();
    let (ops, room) = bridge_ops(&dir.0, lan, tunnel);
    assert!(
        ops.call(ops::BRIDGE_SET, &json!({})).is_err(),
        "on is needed"
    );
    let status = ops.call(ops::BRIDGE_SET, &json!({"on": true})).unwrap();
    assert_eq!(status["state"], "on", "{status}");
    assert_eq!(status["port"], lan);
    assert_eq!(status["fingerprint"].as_str().map(str::len), Some(64));
    assert_eq!(room.named(topics::BRIDGE_CHANGED).len(), 1);
    assert!(listening(lan) && listening(tunnel));
    let pairing = ops.call(ops::BRIDGE_PAIR_BEGIN, &json!({})).unwrap();
    assert_eq!(pairing["code"].as_str().map(str::len), Some(8));
    assert!(pairing["payload"]
        .as_str()
        .unwrap()
        .contains(&format!("\"port\":{lan}")));
    ops.close();
    assert!(!ops.is_on());
    assert!(
        !listening(lan) && !listening(tunnel),
        "closing the daemon closes the ports"
    );
}

#[test]
fn a_pairing_waits_for_the_apps_answer_and_none_is_no() {
    let room = Arc::new(FakeRoom::default());
    let approvals = Arc::new(Approvals::new(room.clone(), Duration::from_secs(5)));
    assert!(
        !approvals.request("Phone", "ios", "192.168.1.40"),
        "no app attached: refused at once"
    );
    assert!(room.named(topics::BRIDGE_PAIR_REQUEST).is_empty());

    room.app.store(true, Ordering::SeqCst);
    assert!(
        !approvals.request("Phone", "ios", "192.168.1.40"),
        "attached but not following the bridge: refused at once"
    );
    assert!(room.named(topics::BRIDGE_PAIR_REQUEST).is_empty());

    room.following.lock().unwrap().push(topics::BRIDGE.into());
    let waiting = {
        let approvals = approvals.clone();
        std::thread::spawn(move || approvals.request("Phone", "ios", "192.168.1.40"))
    };
    let request = loop {
        if let Some(event) = room.named(topics::BRIDGE_PAIR_REQUEST).pop() {
            break event;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(request["name"], "Phone");
    assert_eq!(request["platform"], "ios");
    assert_eq!(request["address"], "192.168.1.40");
    let id = request["request"].as_str().unwrap();
    assert!(!approvals.answer("pair-999", true), "an unknown request");
    assert!(approvals.answer(id, true));
    assert!(waiting.join().unwrap());
    assert!(!approvals.answer(id, true), "answered once");

    let late = Arc::new(Approvals::new(room, Duration::from_millis(50)));
    assert!(
        !late.request("Slow", "android", "10.0.0.2"),
        "unanswered is refused"
    );
}

#[test]
fn a_pairing_is_asked_only_of_an_app_that_follows_the_bridge() {
    use notchling_local_api::{
        Allowlist, Client, ClientKind, ConnectOptions, Limits, Server, ServerConfig,
    };
    let dir = scratch("room");
    let short = PathBuf::from(format!("/tmp/nl-pr-{}", std::process::id()));
    std::fs::create_dir_all(&short).unwrap();
    let socket = short.join("s.sock");
    let room = Arc::new(crate::daemon::LateRoom::default());
    let sessions = Arc::new(Mutex::new(json!([])));
    let setup = BridgeSetup {
        data_dir: dir.0.join("AgentConsole"),
        home: dir.0.join("home"),
        images_dir: dir.0.join("phone-images"),
        projects_dir: dir.0.join("home/.claude/projects"),
        lan_port: 0,
        tunnel_port: 0,
        loopback_only: true,
        approval_wait: Duration::from_secs(10),
    };
    let acting = Arc::new(ActingOps::new(
        Arc::new(Acting::in_memory(false)),
        sessions.clone(),
        Roots {
            main_dir: PathBuf::from("/nonexistent/claude"),
            lanes_file: PathBuf::from("/nonexistent/lanes.json"),
        },
        room.clone(),
        Box::new(NoTerminal),
    ));
    let bridge = Arc::new(BridgeOps::new(
        setup,
        sessions.clone(),
        acting.clone(),
        room.clone(),
        InboxOps::new(dir.0.join("data"), room.clone()),
        Arc::new(EverySession::none()),
    ));
    let handler = crate::daemon::Ops::with_acting(sessions, acting).with_bridge(bridge.clone());
    let server = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: Allowlist::new([std::env::current_exe().unwrap()]),
            events: None,
            version: "9.9.9".into(),
            limits: Limits::default(),
        },
        Arc::new(handler),
    )
    .unwrap();
    let (handle, _thread) = server.spawn().unwrap();
    room.set(handle.clone());

    let options = ConnectOptions {
        check_server: false,
        ..ConnectOptions::default()
    };
    let mut app = Client::connect_with(&socket, ClientKind::App, "0.1.0", &options).unwrap();
    assert!(room.can_answer(), "attached");
    assert!(
        !room.can_follow(topics::BRIDGE),
        "attached is not following the bridge"
    );
    let approvals = bridge.approvals();
    assert!(!approvals.request("Phone", "ios", "192.168.1.40"));

    app.subscribe(&[topics::BRIDGE]).unwrap();
    assert!(room.can_follow(topics::BRIDGE));
    let asking = {
        let approvals = approvals.clone();
        std::thread::spawn(move || approvals.request("Pixel 9", "android", "192.168.1.40"))
    };
    let event = app
        .next_event(Duration::from_secs(5))
        .unwrap()
        .expect("the request");
    assert_eq!(event.event, topics::BRIDGE_PAIR_REQUEST);
    assert_eq!(event.data["address"], "192.168.1.40");
    let answered = app
        .call(
            ops::BRIDGE_PAIR_ANSWER,
            json!({"request": event.data["request"], "allow": false}),
        )
        .unwrap();
    assert_eq!(answered["answered"], true);
    assert!(!asking.join().unwrap(), "Deny is no");

    drop(app);
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&short);
}

fn job_session() -> Session {
    Session::new(
        "s1",
        "n",
        "/tmp/demo",
        Control::Job("abc12345".into()),
        SessionState::Running,
    )
}

#[test]
fn the_bridge_acts_only_through_the_acting_switch() {
    let sessions = Arc::new(Mutex::new(json!([row(
        "job-a",
        json!({"job": "abc12345"}),
        "completed",
        None
    )])));
    let off = DaemonActing {
        ops: acting_ops(false, sessions.clone(), Arc::new(FakeRoom::default())),
        sessions: sessions.clone(),
    };
    let refused = off.send_message("hi", &job_session()).unwrap_err();
    assert_eq!(refused.message, ACTING_OFF);

    let on = DaemonActing {
        ops: acting_ops(true, sessions.clone(), Arc::new(FakeRoom::default())),
        sessions,
    };
    let typed = on.send_message("hi", &job_session()).unwrap_err();
    assert!(
        typed.message.contains("types into no session"),
        "reached the terminal: {typed:?}"
    );
    let view_only = Session::new("s2", "n", "/tmp", Control::ViewOnly, SessionState::Running);
    assert!(on.send_message("hi", &view_only).is_err());
}

#[test]
fn the_phones_short_names_still_map() {
    use crate::bridge_backend::model_id;
    use crate::catalog_ops_tests::seeded;
    use notchling_engine::catalog::LiveModel;

    let (_a, built_in) = seeded("short-built-in", &[], None);
    for (short, id) in [
        ("opus", "claude-opus-5-5"),
        ("sonnet", "claude-sonnet-5-5"),
        ("haiku", "claude-haiku-5-5"),
        ("fable", "claude-fable-5-2"),
    ] {
        assert_eq!(model_id(short, &built_in).as_deref(), Some(id), "{short}");
    }
    assert_eq!(model_id("default", &built_in), None);
    assert_eq!(model_id("evil", &built_in), None);

    let newer = LiveModel {
        id: "claude-fable-5-3".into(),
        ..LiveModel::default()
    };
    let (_b, live) = seeded("short-live", &[newer], None);
    assert_eq!(
        model_id("fable", &live).as_deref(),
        Some("claude-fable-5-3")
    );
    assert_eq!(model_id("opus", &live).as_deref(), Some("claude-opus-5-5"));
}

#[test]
fn bridge_set_pushes_switch_changed_with_its_cause() {
    use crate::event_assert::{of, text};
    use crate::late_emit::Events;
    use notchling_events::{MemorySink, Salt};
    let dir = scratch("switch-events");
    let (ops, _) = bridge_ops(&dir.0, 0, 0);
    let sink = Arc::new(MemorySink::new());
    let ops = ops.with_events(Events::fixed(sink.clone(), Salt([1; 32])));
    ops.call(ops::BRIDGE_SET, &json!({"on": false})).unwrap();
    ops.call(ops::BRIDGE_SET, &json!({"on": false, "restored": true}))
        .unwrap();
    let changed = of(&sink, "switch.changed");
    assert_eq!(changed.len(), 2);
    assert_eq!(text(&changed[0], "switch").as_deref(), Some("bridge"));
    assert_eq!(text(&changed[0], "on").as_deref(), Some("false"));
    assert_eq!(text(&changed[0], "cause").as_deref(), Some("user"));
    assert_eq!(
        text(&changed[1], "cause").as_deref(),
        Some("restored_at_launch")
    );
}
