//! End to end: the bridge switched on over a Swift-made bridge folder (in a temp folder, ports
//! picked by the OS), a phone over TLS pinned to the QR's fingerprint pairs with a one-time code,
//! then `sessions.list` returns the M1 notch fixture's sessions through the live backend. Acting
//! is off, so a message is refused.

use super::bridge::tests::test_config;
use super::bridge::Bridge;
use super::core::tests::no_fetch;
use super::server::tests::{swift_folder, tls_phone_at, Phone};
use crate::bridge::agents;
use crate::bridge::live::{ActingOff, LiveBackend, Sources};
use crate::bridge::model::Session;
use crate::sessions::testutil::TempDir;
use crate::sessions::{Monitor, Probes, Roots};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The M1 notch fixture, read in place (nothing here writes to it).
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/fixtures/notch")
}

/// What the daemon would hand the bridge: its session list, read from the fixture.
struct FixtureSources {
    sessions: Mutex<Vec<Session>>,
}

impl FixtureSources {
    fn read() -> FixtureSources {
        let root = fixture();
        let roots = Roots {
            main_dir: root.join("claude"),
            lanes_file: root.join("notchling/lanes.json"),
        };
        let mut monitor = Monitor::default();
        // A fixture's dates are fixed: keep its finished rows, as the daemon does.
        monitor.completed_window_ms = i64::MAX / 4;
        let rows = monitor.poll(&roots, &Probes::from_proc(), super::files::now_ms());
        FixtureSources {
            sessions: Mutex::new(agents::sessions(&rows)),
        }
    }
}

impl Sources for FixtureSources {
    fn claude_sessions(&self) -> Vec<Session> {
        self.sessions.lock().unwrap().clone()
    }
    fn request_approval(&self, _: &str, _: &str) -> bool {
        true
    }
    fn now_ms(&self) -> f64 {
        super::files::now_ms() as f64
    }
}

fn request(phone: &mut Phone, text: String) -> Value {
    phone.send(tungstenite::Message::text(text)).unwrap();
    loop {
        if let tungstenite::Message::Text(t) = phone.read().unwrap() {
            let reply: Value = serde_json::from_str(t.as_str()).unwrap();
            if reply.get("event").is_none() {
                return reply;
            }
        }
    }
}

#[test]
fn a_phone_pairs_over_tls_and_lists_the_fixture_sessions() {
    let dir = TempDir::new("bridge-e2e");
    swift_folder(&dir);
    let sources = FixtureSources::read();
    let expected: Vec<String> = sources
        .claude_sessions()
        .iter()
        .map(|s| s.id.clone())
        .collect();
    assert!(expected.len() >= 5, "{expected:?}");
    let backend = Arc::new(LiveBackend::new(sources, ActingOff));
    let bridge = Bridge::new(
        test_config(dir.path("AgentConsole"), 0, 0),
        backend,
        no_fetch(),
        Arc::new(super::files::now_ms),
    );
    let status = bridge.set(true);
    assert_eq!(status["state"], "on", "{status}");
    let (lan, _) = bridge.ports().unwrap();
    assert_ne!(lan, super::ports::LAN, "an OS-picked port in tests");

    // What the QR carries: the port, the certificate's fingerprint and the code.
    let pairing = bridge.pair_begin().unwrap();
    let qr: Value = serde_json::from_str(pairing["payload"].as_str().unwrap()).unwrap();
    assert_eq!(qr["port"], lan);
    let mut phone = tls_phone_at(lan, qr["fp"].as_str().unwrap()).unwrap();
    let paired = request(
        &mut phone,
        format!(
            r#"{{"id":1,"op":"pair","code":"{}","device":{{"name":"E2E Phone","platform":"ios"}}}}"#,
            qr["code"].as_str().unwrap()
        ),
    );
    assert_eq!(paired["ok"], true, "{paired}");
    assert!(paired["data"]["token"]
        .as_str()
        .is_some_and(|t| !t.is_empty()));

    let listed = request(&mut phone, r#"{"id":2,"op":"sessions.list"}"#.into());
    assert_eq!(listed["ok"], true, "{listed}");
    let ids: Vec<String> = listed["data"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    let mut want = expected.clone();
    want.sort();
    assert_eq!(sorted, want);
    assert!(ids.contains(&"sess-notch".to_string()), "{ids:?}");
    let row = &listed["data"]["sessions"][ids.iter().position(|i| i == "sess-notch").unwrap()];
    assert_eq!(row["state"], "running", "{row}");
    assert_eq!(row["control"], "job");

    // Acting is off: the bridge reads, it never types.
    let sent = request(
        &mut phone,
        r#"{"id":3,"op":"session.message","sessionId":"sess-notch","text":"hi"}"#.into(),
    );
    assert_eq!(sent["ok"], false, "{sent}");

    // The new phone is in the Swift app's devices.json beside the two it had.
    let devices = bridge.devices().unwrap();
    assert_eq!(devices["devices"].as_array().unwrap().len(), 3);
    assert_eq!(bridge.set(false)["state"], "off");
}
