//! A failed call that types into a session keeps why in its audit line. Before, the line said
//! `failed` and nothing else, so a click that was refused by the screen check (a permission that
//! wasn't on screen, another request in its place, no "don't ask again" option) left no trace of
//! which one.
#![cfg(unix)]

use crate::audit::{audit_reason, REASON_CHARS};
use crate::client::{Client, ConnectOptions};
use crate::server::{Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle};
use crate::testutil::TempDir;
use crate::tier::Allowlist;
use crate::wire::{ApiError, ClientKind, ErrorCode, Tier};
use crate::AuditLog;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

const REFUSAL: &str = "The terminal asks about something else than Chat shows, so nothing was typed. Answer it in the terminal.";

struct Refusing;

impl Handler for Refusing {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        matches!(op, "session.permission" | "accounts.switch").then_some(OpSpec::APP)
    }

    fn topics(&self) -> &[&str] {
        &[]
    }

    fn topic_tier(&self, _topic: &str) -> Tier {
        Tier::Agent
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if args.get("fail") == Some(&json!(false)) {
            return Ok(json!({"decided": "allow"}));
        }
        match op {
            "session.permission" => Err(ApiError::new(ErrorCode::Failed, REFUSAL)),
            _ => Err(ApiError::new(
                ErrorCode::Failed,
                "no login for me@example.com in /Users/me/.claude",
            )),
        }
    }
}

struct Running {
    _dir: TempDir,
    socket: PathBuf,
    audit: PathBuf,
    handle: ServerHandle,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.handle.shutdown();
    }
}

fn start() -> Running {
    let dir = TempDir::new("srv-reason");
    let socket = dir.path().join("run/notchlingd.sock");
    let audit = dir.path().join("audit.jsonl");
    let me = std::env::current_exe().unwrap();
    let server = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: Allowlist::new([me]),
            audit: Some(AuditLog::new(audit.clone())),
            version: "9.9.9".into(),
            limits: Limits::default(),
        },
        Arc::new(Refusing),
    )
    .unwrap();
    let (handle, _thread) = server.spawn().unwrap();
    Running {
        _dir: dir,
        socket,
        audit,
        handle,
    }
}

fn app(run: &Running) -> Client {
    let options = ConnectOptions {
        check_server: false,
        ..ConnectOptions::default()
    };
    Client::connect_with(&run.socket, ClientKind::App, "0.1.0", &options).unwrap()
}

fn audit_lines(run: &Running) -> Vec<Value> {
    std::fs::read_to_string(&run.audit)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn a_failed_answer_into_a_terminal_keeps_why_in_the_audit_line() {
    let run = start();
    let mut client = app(&run);
    assert!(client.call("session.permission", json!({})).is_err());
    assert!(client
        .call("session.permission", json!({"fail": false}))
        .is_ok());
    let lines = audit_lines(&run);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["result"], "failed");
    assert_eq!(lines[0]["reason"], REFUSAL);
    assert_eq!(lines[1]["result"], "ok");
    assert!(
        lines[1].get("reason").is_none(),
        "a call that went through has no reason: {}",
        lines[1]
    );
}

#[test]
fn another_ops_error_text_stays_out_of_the_log() {
    // Their messages may name accounts or folders; only the terminal ops' fixed sentences are kept.
    let run = start();
    let mut client = app(&run);
    assert!(client.call("accounts.switch", json!({})).is_err());
    let lines = audit_lines(&run);
    assert_eq!(lines[0]["result"], "failed");
    assert!(lines[0].get("reason").is_none(), "{}", lines[0]);
}

#[test]
fn a_reason_is_one_capped_line_and_only_for_the_terminal_ops() {
    assert_eq!(
        audit_reason("session.permission", REFUSAL).as_deref(),
        Some(REFUSAL)
    );
    assert_eq!(
        audit_reason("session.reply", "a\nb\r\n c").as_deref(),
        Some("a b c")
    );
    assert_eq!(audit_reason("accounts.switch", "no login for x"), None);
    assert_eq!(audit_reason("session.stop", "folder /x"), None);
    assert_eq!(audit_reason("session.answer", "   "), None);
    // Cut at a character, never inside one, with a mark that it was cut.
    let long = "é".repeat(REASON_CHARS + 50);
    let kept = audit_reason("session.answer", &long).unwrap();
    assert_eq!(kept.chars().count(), REASON_CHARS + 1);
    assert!(kept.ends_with('…') && kept.starts_with("éé"));
}
