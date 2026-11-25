use super::*;
use crate::bridge::net::core::tests::no_fetch;
use crate::bridge::net::jwt::tests::TestClock;
use crate::bridge::net::server::tests::{swift_folder, tls_phone_at, ws_hello, IPHONE_TOKEN};
use crate::bridge::test_support::FakeBackend;
use crate::sessions::testutil::TempDir;
use std::net::TcpStream;
use std::path::Path;
use std::time::SystemTime;

pub(crate) fn free_ports() -> (u16, u16) {
    let a = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let b = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    (
        a.local_addr().unwrap().port(),
        b.local_addr().unwrap().port(),
    )
}

pub(crate) fn test_config(data_dir: PathBuf, lan: u16, tunnel: u16) -> BridgeConfig {
    let parent = data_dir.parent().unwrap_or(&data_dir).to_path_buf();
    let mut config = BridgeConfig::new(data_dir, parent.join("home"), parent.join("phone-images"))
        .with_ports(lan, tunnel);
    config.lan.set_ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
    config.name = "Test Mac".into();
    config.local_host = Some("test-mac.local".into());
    config
}

fn bridge(data_dir: PathBuf, lan: u16, tunnel: u16) -> Bridge {
    Bridge::new(
        test_config(data_dir, lan, tunnel),
        Arc::new(FakeBackend::default()),
        no_fetch(),
        TestClock::new(crate::bridge::net::files::now_ms()).clock(),
    )
}

pub(crate) fn snapshot(dir: &Path) -> Vec<(PathBuf, u64, Option<SystemTime>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
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

fn refused(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_err()
}

#[test]
fn off_binds_no_port_and_on_binds_both() {
    let dir = TempDir::new("bridge-ports");
    swift_folder(&dir);
    let (lan, tunnel) = free_ports();
    let bridge = bridge(dir.path("AgentConsole"), lan, tunnel);
    assert_eq!(bridge.status()["state"], "off");
    assert_eq!(bridge.ports(), None);
    assert!(refused(lan) && refused(tunnel), "nothing listens while off");
    let _ = bridge.devices();
    let _ = bridge.tunnel_status();
    assert!(
        refused(lan) && refused(tunnel),
        "asking for state binds nothing"
    );

    let status = bridge.set(true);
    assert_eq!(status["state"], "on", "{status}");
    assert_eq!(status["port"], lan);
    assert_eq!(bridge.ports(), Some((lan, tunnel)));
    assert!(!refused(lan) && !refused(tunnel));

    assert_eq!(bridge.set(false)["state"], "off");
    assert!(refused(lan) && refused(tunnel), "off closes both");
}

#[test]
fn nothing_under_the_data_folder_is_written_while_off() {
    let dir = TempDir::new("bridge-nowrite");
    swift_folder(&dir);
    let data = dir.path("AgentConsole");
    let before = snapshot(&data);
    let (lan, tunnel) = free_ports();
    let bridge = bridge(data.clone(), lan, tunnel);
    bridge.status();
    assert_eq!(
        bridge.devices().unwrap()["devices"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(bridge.tunnel_status()["enabled"], true);
    assert!(bridge.pair_begin().is_err(), "pairing needs the bridge on");
    assert!(
        bridge
            .revoke("6D0F2A58-3B77-4C0E-9A41-5E8B1C7D2F90")
            .is_err(),
        "revoking writes, so not while off"
    );
    bridge.set(false);
    assert_eq!(snapshot(&data), before);

    let empty = TempDir::new("bridge-empty");
    let data = empty.path("AgentConsole");
    let bridge = super::tests::bridge(data.clone(), lan, tunnel);
    let status = bridge.set(true);
    assert_eq!(status["state"], "failed");
    assert!(
        status["error"].as_str().unwrap().contains("Swift app"),
        "{status}"
    );
    assert!(!data.exists(), "no certificate is generated");
    assert!(refused(lan) && refused(tunnel));
}

#[test]
fn a_busy_port_is_an_error_not_another_port() {
    let dir = TempDir::new("bridge-busy");
    swift_folder(&dir);
    let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = busy.local_addr().unwrap().port();
    let (_, tunnel) = free_ports();
    let bridge = bridge(dir.path("AgentConsole"), port, tunnel);
    let status = bridge.set(true);
    assert_eq!(status["state"], "failed");
    assert!(status["error"]
        .as_str()
        .unwrap()
        .contains(&port.to_string()));
    assert!(refused(tunnel), "the tunnel port isn't left open either");
}

#[test]
fn replies_have_the_panels_shapes() {
    let dir = TempDir::new("bridge-shapes");
    swift_folder(&dir);
    let (lan, tunnel) = free_ports();
    let bridge = bridge(dir.path("AgentConsole"), lan, tunnel);
    let devices = bridge.devices().unwrap();
    let first = &devices["devices"][0];
    assert_eq!(first["id"], "6D0F2A58-3B77-4C0E-9A41-5E8B1C7D2F90");
    assert_eq!(first["platform"], "ios");
    assert!(first["paired_at"].as_i64().unwrap() > 1_700_000_000_000);
    assert!(devices["devices"][1]["last_seen"].is_null());

    let tunnel_off = bridge.tunnel_status();
    assert_eq!(tunnel_off["configured"], true);
    assert_eq!(tunnel_off["connected"], false);
    assert!(tunnel_off.get("clientSecret").is_none() && !tunnel_off.to_string().contains("\"s\""));

    bridge.set(true);
    let pairing = bridge.pair_begin().unwrap();
    let code = pairing["code"].as_str().unwrap();
    assert_eq!(code.len(), 8);
    assert!(pairing["expires_at"].as_i64().unwrap() > 0);
    let hosts = pairing["hosts"].as_array().unwrap();
    assert_eq!(hosts.last().unwrap(), "test-mac.local");
    let payload: Value = serde_json::from_str(pairing["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["code"], code);
    assert_eq!(payload["port"], lan);
    assert_eq!(payload["fp"], bridge.status()["fingerprint"]);
    assert_eq!(payload["name"], "Test Mac");
    assert_eq!(payload["tunnel"]["url"], "wss://h.example");
}

#[test]
fn a_phone_pairs_with_the_code_and_revoke_drops_it() {
    let dir = TempDir::new("bridge-pair");
    swift_folder(&dir);
    let (lan, tunnel) = free_ports();
    let bridge = bridge(dir.path("AgentConsole"), lan, tunnel);
    bridge.set(true);
    let fingerprint = bridge.status()["fingerprint"].as_str().unwrap().to_string();
    let code = bridge.pair_begin().unwrap()["code"]
        .as_str()
        .unwrap()
        .to_string();
    let mut phone = tls_phone_at(lan, &fingerprint).unwrap();
    let pair = format!(
        r#"{{"id":1,"op":"pair","code":"{code}","device":{{"name":"New Phone","platform":"android"}}}}"#
    );
    phone.send(tungstenite::Message::text(pair)).unwrap();
    let reply: Value = match phone.read().unwrap() {
        tungstenite::Message::Text(t) => serde_json::from_str(t.as_str()).unwrap(),
        other => panic!("{other:?}"),
    };
    assert_eq!(reply["ok"], true, "{reply}");
    let id = reply["data"]["deviceId"].as_str().unwrap().to_string();
    let listed = bridge.devices().unwrap();
    assert_eq!(listed["devices"].as_array().unwrap().len(), 3);
    let on_disk =
        crate::bridge::net::devices::read_devices(&dir.path("AgentConsole/bridge/devices.json"))
            .unwrap();
    assert_eq!(on_disk.len(), 3, "kept in the Swift app's file");

    assert_eq!(bridge.revoke(&id).unwrap()["revoked"], true);
    let gone = loop {
        match phone.read() {
            Ok(tungstenite::Message::Close(_)) | Err(_) => break true,
            Ok(_) => continue,
        }
    };
    assert!(gone);
    assert_eq!(bridge.revoke(&id).unwrap()["revoked"], false);
    let mut swift_phone = tls_phone_at(lan, &fingerprint).unwrap();
    assert_eq!(
        ws_hello(&mut swift_phone, IPHONE_TOKEN)["ok"],
        true,
        "the Swift app's phones still connect"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while bridge.status()["connections"] != 1 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(bridge.status()["connections"], 1, "the revoked one is gone");
}
