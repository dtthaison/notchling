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
            let modified = if cfg!(windows) && meta.is_dir() {
                None
            } else {
                meta.modified().ok()
            };
            out.push((e.path(), meta.len(), modified));
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

fn with_outbox(acting: Arc<ActingOps>) -> Arc<ActingOps> {
    let outbox = crate::outbox_ops::OutboxOps::new(
        Arc::new(notchling_engine::outbox::OutboxStore::in_memory_for_tests()),
        acting.sessions.clone(),
        acting.roots.clone(),
        acting.acting_switch(),
        acting.room.clone(),
        None,
    );
    acting.attach_outbox(Arc::new(outbox));
    acting
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

#[cfg(unix)]
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
        ops: with_outbox(acting_ops(
            false,
            sessions.clone(),
            Arc::new(FakeRoom::default()),
        )),
        sessions: sessions.clone(),
        cluster: crate::bridge_cluster::PhoneCluster::none(),
    };
    let refused = off.send_message("hi", &job_session()).unwrap_err();
    assert_eq!(refused.message, ACTING_OFF);

    let on = DaemonActing {
        ops: with_outbox(acting_ops(
            true,
            sessions.clone(),
            Arc::new(FakeRoom::default()),
        )),
        sessions,
        cluster: crate::bridge_cluster::PhoneCluster::none(),
    };
    on.send_message("hi", &job_session()).unwrap();
    let outbox = on.ops.outbox().unwrap();
    assert_eq!(outbox.store().list(Some("abc12345")).unwrap().len(), 1);
    let view_only = Session::new("s2", "n", "/tmp", Control::ViewOnly, SessionState::Running);
    assert!(on.send_message("hi", &view_only).is_err());
}

struct Counting(Arc<std::sync::atomic::AtomicUsize>);

impl crate::cluster_forward::Forward for Counting {
    fn forward(
        &self,
        _: &notchling_local_api::Caller,
        _: &str,
        _: &Value,
    ) -> Option<Result<Value, notchling_local_api::ApiError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Some(Ok(json!({"forwarded": true})))
    }
}

struct RemoteWorld {
    handler: Arc<dyn notchling_local_api::Handler>,
    cluster: Arc<crate::bridge_cluster::PhoneCluster>,
    sent: Arc<std::sync::atomic::AtomicUsize>,
    acting: Arc<ActingOps>,
    _dir: Scratch,
}

fn remote_world(on: bool) -> RemoteWorld {
    let sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let sessions = Arc::new(Mutex::new(json!([])));
    let acting = acting_ops(on, sessions.clone(), Arc::new(FakeRoom::default()));
    let dir = scratch("forward-acting");
    let remote = Arc::new(crate::cluster_sessions::RemoteSessions::new(
        dir.0.join("mirror"),
    ));
    let handler: Arc<dyn notchling_local_api::Handler> = Arc::new(
        crate::daemon::Ops::with_acting(sessions, acting.clone())
            .with_cluster(Some(Arc::new(Counting(sent.clone()))), remote.clone()),
    );
    let late = Arc::new(crate::cluster_switches::LateHandler::default());
    late.serve(&handler);
    let cluster = crate::bridge_cluster::PhoneCluster::none();
    let prompts =
        crate::cluster_prompts::RemotePrompts::new(Arc::new(FakeRoom::default()), remote.clone());
    cluster.attach(late, remote, prompts);
    RemoteWorld {
        handler,
        cluster,
        sent,
        acting,
        _dir: dir,
    }
}

fn the_app() -> notchling_local_api::Caller {
    notchling_local_api::Caller {
        tier: notchling_local_api::Tier::App,
        client: notchling_local_api::ClientKind::App,
        peer: None,
        pid: None,
    }
}

#[test]
fn forward_refused_while_acting_off() {
    use notchling_local_api::{ops, ErrorCode};
    const REMOTE: &str = "0123456789abcdef/job-7";
    let off = remote_world(false);
    for (op, args) in [
        (ops::SESSION_REPLY, json!({"id": REMOTE, "text": "hi"})),
        (ops::SESSION_INTERRUPT, json!({"id": REMOTE})),
        (
            ops::SESSION_PERMISSION,
            json!({"id": REMOTE, "decision": "allow"}),
        ),
        (ops::SESSION_ANSWER, json!({"id": REMOTE, "answers": []})),
        (ops::CODEX_SEND, json!({"id": REMOTE, "text": "hi"})),
        (ops::AGY_SEND, json!({"id": REMOTE, "text": "hi"})),
    ] {
        let refused = off.handler.call(&the_app(), op, &args).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden, "{op}");
        assert_eq!(refused.message, ACTING_OFF, "{op}");
    }
    assert_eq!(off.sent.load(Ordering::SeqCst), 0, "nothing was forwarded");

    let phone = DaemonActing {
        ops: off.acting.clone(),
        sessions: Arc::new(Mutex::new(json!([]))),
        cluster: off.cluster.clone(),
    };
    let mut remote = Session::new(
        REMOTE,
        "n",
        "/tmp",
        Control::Job(REMOTE.into()),
        SessionState::Running,
    );
    remote.node = Some(notchling_engine::bridge::model::SessionNode {
        id: "0123456789abcdef".into(),
        name: "mac-mini".into(),
        online: true,
    });
    assert_eq!(
        phone.send_message("hi", &remote).unwrap_err().message,
        ACTING_OFF
    );
    assert_eq!(phone.send_now(&remote).unwrap_err().message, ACTING_OFF);
    let prompt = notchling_engine::bridge::model::PendingPrompt {
        id: "0123456789abcdef/p1".into(),
        session_id: REMOTE.into(),
        kind: notchling_engine::bridge::model::PromptKind::Permission,
        tool_name: "Bash".into(),
        summary: "ls".into(),
        created_ms: 0.0,
    };
    let denied = phone
        .respond(
            &prompt,
            &notchling_engine::bridge::model::PromptResponse::Allow,
        )
        .unwrap_err();
    assert_eq!(denied.message, ACTING_OFF);

    let on = remote_world(true);
    on.handler
        .call(
            &the_app(),
            ops::SESSION_REPLY,
            &json!({"id": REMOTE, "text": "hi"}),
        )
        .unwrap();
    assert_eq!(on.sent.load(Ordering::SeqCst), 1, "with acting on it goes");
}

#[test]
fn shell_stop_and_an_app_dispatch_are_refused_while_acting_off() {
    use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode, Tier};
    const REMOTE_JOB: &str = "0123456789abcdef/shell:j1";
    let agent = Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: None,
    };
    let node = Caller {
        client: ClientKind::Node,
        ..agent.clone()
    };
    let is_off = |e: ApiError| e.code == ErrorCode::Forbidden && e.message == ACTING_OFF;
    let dispatch = json!({
        "node": "0123456789abcdef", "key": "k", "tool": "shell", "cwd": "/w", "command": "make",
    });
    let delegate = json!({"node": "0123456789abcdef", "key": "k", "delegate": ["codex"]});

    let off = remote_world(false);
    for id in [REMOTE_JOB, "shell:j1"] {
        let refused = off
            .handler
            .call(&the_app(), ops::SHELL_STOP, &json!({"id": id}))
            .unwrap_err();
        assert!(is_off(refused), "{id}");
    }
    let refused = off
        .handler
        .call(&the_app(), ops::CLUSTER_DISPATCH, &dispatch)
        .unwrap_err();
    assert!(is_off(refused), "an app dispatch starts work on a Worker");
    assert_eq!(off.sent.load(Ordering::SeqCst), 0, "nothing was forwarded");
    let other = off
        .handler
        .call(&agent, ops::CLUSTER_DISPATCH, &delegate)
        .err();
    assert!(
        !other.is_some_and(is_off),
        "an agent's delegate --machine is not an acting op"
    );
    let from_node = off
        .handler
        .call(&node, ops::SHELL_STOP, &json!({"id": "shell:j1"}))
        .err();
    assert!(
        !from_node.is_some_and(is_off),
        "a node's own call is the owner's to gate"
    );

    let on = remote_world(true);
    on.handler
        .call(&the_app(), ops::SHELL_STOP, &json!({"id": REMOTE_JOB}))
        .unwrap();
    assert_eq!(on.sent.load(Ordering::SeqCst), 1, "with acting on it goes");
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

#[test]
fn the_served_probe_reads_a_cache_set_when_the_bridge_starts_or_stops() {
    let dir = scratch("served");
    swift_folder(&dir.0.join("AgentConsole"));
    let (lan, tunnel) = free_ports();
    let (ops, _) = bridge_ops(&dir.0, lan, tunnel);
    let off = ops.served();
    assert_eq!((off.port, off.fingerprint.clone()), (lan, None));
    ops.call(ops::BRIDGE_SET, &json!({"on": true})).unwrap();
    let on = ops.served();
    assert_eq!(on.port, lan);
    assert_eq!(on.fingerprint.as_deref().map(str::len), Some(64));
    ops.call(ops::BRIDGE_SET, &json!({"on": false})).unwrap();
    assert_eq!(ops.served(), off);
    ops.call(ops::BRIDGE_SET, &json!({"on": true})).unwrap();
    assert_eq!(ops.served(), on);
    ops.close();
    assert_eq!(ops.served(), off);
}

#[test]
fn a_bridge_that_fails_to_start_serves_no_fingerprint() {
    let dir = scratch("served-failed");
    let (lan, tunnel) = free_ports();
    let (ops, _) = bridge_ops(&dir.0, lan, tunnel);
    let status = ops.call(ops::BRIDGE_SET, &json!({"on": true})).unwrap();
    assert_eq!(status["state"], "failed", "{status}");
    assert_eq!(ops.served().fingerprint, None);
}

#[test]
fn the_served_probe_never_waits_on_the_bridge() {
    let dir = scratch("served-wait");
    swift_folder(&dir.0.join("AgentConsole"));
    let (lan, tunnel) = free_ports();
    let (ops, _) = bridge_ops(&dir.0, lan, tunnel);
    let ops = Arc::new(ops);
    ops.call(ops::BRIDGE_SET, &json!({"on": true})).unwrap();
    let flip = {
        let ops = ops.clone();
        std::thread::spawn(move || {
            for n in 0..6 {
                let _ = ops.call(ops::BRIDGE_SET, &json!({"on": n % 2 == 1}));
            }
        })
    };
    for _ in 0..2_000 {
        assert_eq!(ops.served().port, lan);
    }
    flip.join().unwrap();
}
