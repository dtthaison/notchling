#![cfg(any(target_os = "macos", target_os = "linux"))]

use notchling_local_api::{
    Allowlist, Client, ClientKind, ConnectOptions, Limits, OpSpec, Server, ServerConfig, Tier,
};
use notchling_local_api::{ApiError, Caller, Handler};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

const SOCKET_ENV: &str = "NL_PEER_SOCKET";

struct Nothing;

impl Handler for Nothing {
    fn spec(&self, _op: &str) -> Option<OpSpec> {
        None
    }
    fn call(&self, _: &Caller, _: &str, _: &Value) -> Result<Value, ApiError> {
        Ok(Value::Null)
    }
}

#[test]
#[ignore]
fn peer_helper() {
    let Some(socket) = std::env::var_os(SOCKET_ENV) else {
        return;
    };
    let options = ConnectOptions {
        check_server: false,
        ..ConnectOptions::default()
    };
    let c = Client::connect_with(Path::new(&socket), ClientKind::App, "0.1.0", &options).unwrap();
    let tier = match c.tier() {
        Tier::App => "app",
        Tier::Agent => "agent",
    };
    println!("TIER={tier}");
}

fn tier_of_child(exe: &Path, socket: &Path) -> String {
    let out = Command::new(exe)
        .args([
            "--exact",
            "peer_helper",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SOCKET_ENV, socket)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.lines()
        .find_map(|l| l.split("TIER=").nth(1).map(|t| t.trim().to_string()))
        .unwrap_or_else(|| panic!("no tier from {}: {text}", exe.display()))
}

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_copy_is_the_agent_tier_and_a_symlink_is_the_app() {
    let dir = Dir(PathBuf::from(format!(
        "/tmp/nl-peer-{}",
        std::process::id()
    )));
    let _ = std::fs::remove_dir_all(&dir.0);
    std::fs::create_dir_all(&dir.0).unwrap();
    let me = std::env::current_exe().unwrap();
    let socket = dir.0.join("notchlingd.sock");
    let server = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: Allowlist::new([&me]),
            audit: None,
            version: "0.1.0".into(),
            limits: Limits::default(),
        },
        Arc::new(Nothing),
    )
    .unwrap();
    let (handle, thread) = server.spawn().unwrap();

    assert_eq!(tier_of_child(&me, &socket), "app");

    let copy = dir.0.join("copy-of-app");
    std::fs::copy(&me, &copy).unwrap();
    assert_eq!(tier_of_child(&copy, &socket), "agent");

    let link = dir.0.join("link-to-app");
    std::os::unix::fs::symlink(&me, &link).unwrap();
    assert_eq!(tier_of_child(&link, &socket), "app");

    handle.shutdown();
    thread.join().unwrap().unwrap();
}
