//! The pairing panel's readers (`phone::wire`) on the engine bridge's real replies: a bridge folder
//! laid out as the Swift app leaves it (test certificate, its `devices.json` and `access.json`) in
//! a temp folder, ports picked by the OS, no network.

use notchling_app::phone::wire::{self, BridgeState, TunnelState};
use notchling_engine::bridge::live::{ActingOff, LiveBackend, Sources};
use notchling_engine::bridge::model::Session;
use notchling_engine::bridge::net::{Bridge, BridgeConfig};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const DEVICES: &str = include_str!("../../engine/src/bridge/net/testdata/devices.swift.json");

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch() -> Scratch {
    let dir = std::env::temp_dir().join(format!(
        "notchling-phone-replies-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

/// `identity.p12` + `identity.pass` + `cert.pem`, as `BridgeIdentity.generate` leaves them.
fn swift_bridge_folder(bridge: &Path) {
    std::fs::create_dir_all(bridge).unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["Agent Console (test)".to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    let pass = "pass-for-the-test";
    let pfx = p12::PFX::new(
        cert.der(),
        &key.serialize_der(),
        None,
        pass,
        "Agent Console",
    )
    .unwrap();
    std::fs::write(bridge.join("identity.p12"), pfx.to_der()).unwrap();
    std::fs::write(bridge.join("identity.pass"), pass).unwrap();
    std::fs::write(bridge.join("cert.pem"), cert.pem()).unwrap();
    std::fs::write(bridge.join("devices.json"), DEVICES).unwrap();
    let access = serde_json::json!({"enabled": true, "hostname": "h.example",
        "teamDomain": "team.cloudflareaccess.com", "audience": "aud", "clientId": "id",
        "clientSecret": "s"});
    std::fs::write(bridge.join("access.json"), access.to_string()).unwrap();
}

struct NoSessions;

impl Sources for NoSessions {
    fn claude_sessions(&self) -> Vec<Session> {
        Vec::new()
    }
    fn now_ms(&self) -> f64 {
        0.0
    }
}

#[test]
fn the_panel_reads_every_reply_the_bridge_gives() {
    let dir = scratch();
    let data = dir.0.join("AgentConsole");
    swift_bridge_folder(&data.join("bridge"));
    let mut config =
        BridgeConfig::new(data, dir.0.join("home"), dir.0.join("phone-images")).with_ports(0, 0);
    config.lan.set_ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let bridge = Bridge::new(
        config,
        Arc::new(LiveBackend::new(NoSessions, ActingOff)),
        Arc::new(|_: &str| Err("no network in tests".to_string())),
        Arc::new(|| 1_793_541_465_000),
    );

    let off = wire::parse_status(&bridge.status()).unwrap();
    assert_eq!(off.state, BridgeState::Off);
    let devices = wire::parse_devices(&bridge.devices().unwrap());
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].id, "6D0F2A58-3B77-4C0E-9A41-5E8B1C7D2F90");
    assert_eq!(devices[1].name, "Pixel 9 / work");
    assert!(devices[0].paired_at_ms.is_some() && devices[1].last_seen_ms.is_none());
    let tunnel = wire::parse_tunnel(&bridge.tunnel_status()).unwrap();
    assert_eq!(
        tunnel.state,
        TunnelState::Waiting,
        "on and configured, nothing through yet"
    );

    let on = wire::parse_status(&bridge.set(true)).unwrap();
    let (port, _) = bridge.ports().unwrap();
    assert_eq!(on.state, BridgeState::On(Some(port)));
    assert_eq!(on.fingerprint.as_deref().map(str::len), Some(64));
    let pairing = wire::parse_pairing(&bridge.pair_begin().unwrap()).unwrap();
    assert_eq!(pairing.code.len(), 8);
    assert!(pairing.payload.contains(&pairing.code));
    assert_eq!(pairing.expires_at_ms, 1_793_541_465_000 + 300_000);

    assert_eq!(
        wire::parse_status(&bridge.set(false)).unwrap().state,
        BridgeState::Off
    );
    // A folder without the Swift app's certificate: the panel shows why.
    let empty = scratch();
    let bridge = Bridge::new(
        BridgeConfig::new(
            empty.0.join("AgentConsole"),
            empty.0.join("home"),
            empty.0.join("img"),
        )
        .with_ports(0, 0),
        Arc::new(LiveBackend::new(NoSessions, ActingOff)),
        Arc::new(|_: &str| Err("no network in tests".to_string())),
        Arc::new(|| 0),
    );
    match wire::parse_status(&bridge.set(true)).unwrap().state {
        BridgeState::Failed(why) => assert!(why.contains("certificate"), "{why}"),
        other => panic!("{other:?}"),
    }
}
