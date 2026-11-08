//! The bridge ops called directly over a temp `AgentConsole` folder laid out as the Swift app
//! leaves it: off at the start binds no port and writes nothing, the app's replies, pairing
//! approval through the room, and the bridge's actions going through the acting switch.

use super::*;
use crate::acting::{Acting, ACTING_OFF};
use crate::acting_ops::NoTerminal;
use crate::acting_ops_tests::{row, FakeRoom};
use crate::bridge_backend::{Approvals, DaemonActing};
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

/// The Swift app's bridge folder: identity, two paired phones, internet access configured.
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

/// Two ports nobody listens on now.
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
    (BridgeOps::new(setup, sessions, acting, room.clone()), room)
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
        !approvals.request("Phone", "ios"),
        "no app attached: refused at once"
    );
    assert!(room.named(topics::BRIDGE_PAIR_REQUEST).is_empty());

    room.app.store(true, Ordering::SeqCst);
    let waiting = {
        let approvals = approvals.clone();
        std::thread::spawn(move || approvals.request("Phone", "ios"))
    };
    let request = loop {
        if let Some(event) = room.named(topics::BRIDGE_PAIR_REQUEST).pop() {
            break event;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(request["name"], "Phone");
    let id = request["request"].as_str().unwrap();
    assert!(!approvals.answer("pair-999", true), "an unknown request");
    assert!(approvals.answer(id, true));
    assert!(waiting.join().unwrap());
    assert!(!approvals.answer(id, true), "answered once");

    let late = Arc::new(Approvals::new(room, Duration::from_millis(50)));
    assert!(!late.request("Slow", "android"), "unanswered is refused");
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
        "working",
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
