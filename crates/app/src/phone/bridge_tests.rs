//! The bridge's tests, over a fake transport.

use super::*;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::json;
use std::io;
use std::sync::Arc;

type Calls = Arc<Mutex<Vec<(String, Value)>>>;
type Script = Box<dyn Fn(&str, &Value) -> Result<Value, ClientError> + Send + Sync>;

/// Answers each op from a script and records every call.
struct Fake {
    calls: Calls,
    answer: Script,
}

impl Transport for Fake {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        let result = (self.answer)(op, &args);
        lock(&self.calls).push((op.to_owned(), args));
        result
    }
}

/// Remembers the user's choice in memory.
struct Memory(Arc<Mutex<bool>>);

impl Choice for Memory {
    fn get(&self) -> bool {
        *lock(&self.0)
    }
    fn set(&self, on: bool) -> Result<(), String> {
        *lock(&self.0) = on;
        Ok(())
    }
}

fn fake(
    answer: impl Fn(&str, &Value) -> Result<Value, ClientError> + Send + Sync + 'static,
) -> (Bridge, Calls, Arc<Mutex<bool>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let chosen = Arc::new(Mutex::new(false));
    let bridge = Bridge::over(Box::new(Fake {
        calls: calls.clone(),
        answer: Box::new(answer),
    }))
    .with_choice(Box::new(Memory(chosen.clone())));
    (bridge, calls, chosen)
}

fn ops_of(calls: &Calls) -> Vec<String> {
    lock(calls).iter().map(|(op, _)| op.clone()).collect()
}

fn gone() -> ClientError {
    ClientError::NotRunning(io::Error::from(io::ErrorKind::NotFound))
}

/// A daemon with the bridge in the given state and two phones.
fn daemon(state: &'static str) -> impl Fn(&str, &Value) -> Result<Value, ClientError> {
    move |op, _| {
        Ok(match op {
            "bridge.status" | "bridge.set" => json!({"state": state, "port": 47821}),
            "bridge.devices" => json!({"devices": [
                {"id": "d1", "name": "Pixel", "platform": "android"},
                {"id": "d2", "name": "iPhone", "platform": "ios"}]}),
            "bridge.tunnel.status" => json!({"enabled": false}),
            "bridge.pair.begin" => json!({"code": "K7M4XQ2P", "payload": "{\"v\":1}",
                                          "expires_at": 1_800_000_300_000_i64}),
            _ => json!({}),
        })
    }
}

#[test]
fn nothing_is_known_until_the_daemon_answers() {
    let (bridge, calls, _) = fake(daemon("running"));
    assert_eq!(bridge.snapshot(), Snapshot::detached());
    assert!(calls.lock().map(|c| c.is_empty()).unwrap_or(false));
}

#[test]
fn refresh_reads_status_phones_and_tunnel() {
    let (bridge, calls, _) = fake(daemon("running"));
    bridge.refresh(0);
    let s = bridge.snapshot();
    assert!(s.attached);
    assert_eq!(s.state, BridgeState::On(Some(47821)));
    assert_eq!(s.devices.len(), 2);
    assert!(s.tunnel.is_some());
    assert_eq!(s.error, None);
    assert_eq!(
        ops_of(&calls),
        ["bridge.status", "bridge.devices", "bridge.tunnel.status"]
    );
}

#[test]
fn a_choice_of_on_is_sent_again_to_a_daemon_that_came_up_off() {
    let (bridge, calls, chosen) = fake(|op, _| match op {
        "bridge.status" => Ok(json!({"state": "off"})),
        "bridge.set" => Ok(json!({"state": "running", "port": 47821})),
        _ => Ok(json!({"devices": []})),
    });
    // The user never chose: the daemon stays off and is not asked.
    bridge.refresh(0);
    assert_eq!(bridge.snapshot().state, BridgeState::Off);
    assert!(!ops_of(&calls).contains(&"bridge.set".to_owned()));
    *lock(&chosen) = true;
    bridge.refresh(1);
    assert_eq!(bridge.snapshot().state, BridgeState::On(Some(47821)));
    let sent = lock(&calls)
        .iter()
        .find(|(op, _)| op == "bridge.set")
        .map(|(_, a)| a.clone());
    assert_eq!(sent, Some(json!({"on": true})));
}

#[test]
fn a_refused_choice_is_not_retried_every_poll() {
    let (bridge, calls, chosen) = fake(|op, _| match op {
        "bridge.status" => Ok(json!({"state": "off"})),
        "bridge.set" => Err(ClientError::Api(ApiError::new(
            ErrorCode::Failed,
            "port 47821 is busy",
        ))),
        _ => Ok(json!({"devices": []})),
    });
    *lock(&chosen) = true;
    bridge.refresh(1_000);
    let s = bridge.snapshot();
    assert_eq!(s.state, BridgeState::Off);
    assert_eq!(s.error.as_deref(), Some("port 47821 is busy"));
    let sets = |c: &Calls| ops_of(c).iter().filter(|o| *o == "bridge.set").count();
    assert_eq!(sets(&calls), 1);
    bridge.refresh(2_000);
    assert_eq!(sets(&calls), 1, "inside the wait");
    bridge.refresh(1_000 + RETRY_AFTER_MS);
    assert_eq!(sets(&calls), 2, "after the wait");
}

#[test]
fn a_lost_daemon_detaches_and_forgets() {
    let up = Arc::new(Mutex::new(true));
    let flag = up.clone();
    let inner = daemon("running");
    let (bridge, _, _) = fake(move |op, args| {
        if *lock(&flag) {
            inner(op, args)
        } else {
            Err(gone())
        }
    });
    bridge.refresh(0);
    assert!(bridge.snapshot().attached);
    *lock(&up) = false;
    bridge.refresh(1);
    assert_eq!(bridge.snapshot(), Snapshot::detached());
}

#[test]
fn a_daemon_without_the_bridge_is_attached_with_a_reason() {
    let (bridge, _, _) = fake(|_, _| {
        Err(ClientError::Api(ApiError::new(
            ErrorCode::UnknownOp,
            "unknown op bridge.status",
        )))
    });
    bridge.refresh(0);
    let s = bridge.snapshot();
    assert!(s.attached, "a refusal is an answer");
    assert_eq!(s.state, BridgeState::Off);
    assert_eq!(s.error.as_deref(), Some("unknown op bridge.status"));
}

#[test]
fn choosing_keeps_the_choice_and_tells_the_daemon() {
    let (bridge, calls, chosen) = fake(daemon("running"));
    assert_eq!(bridge.choose(true), Ok(()));
    assert!(*lock(&chosen));
    assert!(bridge.snapshot().state.is_on());
    assert_eq!(
        lock(&calls).first().cloned(),
        Some(("bridge.set".to_owned(), json!({"on": true})))
    );
    let (bridge, _, chosen) = fake(daemon("off"));
    *lock(&chosen) = true;
    assert_eq!(bridge.choose(false), Ok(()));
    assert!(!*lock(&chosen));
    assert_eq!(bridge.snapshot().state, BridgeState::Off);
}

#[test]
fn turning_the_bridge_off_drops_the_code_on_show() {
    let (bridge, _, _) = fake(|op, a| match op {
        "bridge.set" => {
            Ok(json!({"state": if a["on"] == json!(true) { "running" } else { "off" }}))
        }
        other => daemon("running")(other, a),
    });
    bridge.refresh(0);
    assert_eq!(bridge.begin_pairing(), Ok(()));
    assert!(bridge.snapshot().pairing.is_some());
    assert_eq!(bridge.choose(false), Ok(()));
    assert_eq!(bridge.snapshot().pairing, None);
}

#[test]
fn a_refused_switch_says_why_and_keeps_the_state() {
    let (bridge, _, _) = fake(|op, _| match op {
        "bridge.set" => Err(ClientError::Api(ApiError::new(
            ErrorCode::Failed,
            "port 47821 is busy",
        ))),
        _ => Ok(json!({"state": "off"})),
    });
    bridge.refresh(0);
    assert_eq!(bridge.choose(true), Err("port 47821 is busy".to_owned()));
    let s = bridge.snapshot();
    assert_eq!(s.state, BridgeState::Off);
    assert_eq!(s.error.as_deref(), Some("port 47821 is busy"));
}

#[test]
fn pairing_and_revoking_are_refused_unsent_while_the_bridge_is_off() {
    let (bridge, calls, _) = fake(daemon("off"));
    assert_eq!(bridge.begin_pairing(), Err(DETACHED_MESSAGE.to_owned()));
    bridge.refresh(0);
    let before = lock(&calls).len();
    assert_eq!(bridge.begin_pairing(), Err(OFF_MESSAGE.to_owned()));
    assert_eq!(bridge.revoke("d1"), Err(OFF_MESSAGE.to_owned()));
    assert_eq!(lock(&calls).len(), before, "nothing reached the daemon");
}

#[test]
fn pairing_keeps_the_code_and_revoking_removes_the_phone() {
    let (bridge, calls, _) = fake(daemon("running"));
    bridge.refresh(0);
    assert_eq!(bridge.begin_pairing(), Ok(()));
    let pairing = bridge.snapshot().pairing;
    assert_eq!(pairing.as_ref().map(|p| p.code.as_str()), Some("K7M4XQ2P"));
    bridge.close_pairing();
    assert_eq!(bridge.snapshot().pairing, None);
    assert_eq!(bridge.revoke("d1"), Ok(()));
    let ids: Vec<String> = bridge
        .snapshot()
        .devices
        .into_iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(ids, ["d2"]);
    let revoke = lock(&calls)
        .iter()
        .find(|(op, _)| op == "bridge.revoke")
        .map(|(_, a)| a.clone());
    assert_eq!(revoke, Some(json!({"device": "d1"})));
}

#[test]
fn a_pairing_the_app_cannot_read_is_an_error() {
    let (bridge, _, _) = fake(|op, a| match op {
        "bridge.pair.begin" => Ok(json!({"code": "ABCD"})),
        other => daemon("running")(other, a),
    });
    bridge.refresh(0);
    assert!(bridge.begin_pairing().is_err());
    assert_eq!(bridge.snapshot().pairing, None);
    assert!(bridge.snapshot().error.is_some());
}

#[test]
fn a_bridge_that_goes_off_by_itself_drops_the_code() {
    let state = Arc::new(Mutex::new("running"));
    let seen = state.clone();
    let (bridge, _, _) = fake(move |op, a| match op {
        "bridge.status" => Ok(json!({"state": *lock(&seen)})),
        other => daemon("running")(other, a),
    });
    bridge.refresh(0);
    assert_eq!(bridge.begin_pairing(), Ok(()));
    *lock(&state) = "off";
    bridge.refresh(1);
    assert_eq!(bridge.snapshot().pairing, None);
}

#[test]
fn a_fixed_bridge_never_calls_and_refuses() {
    let bridge = Bridge::fixed(Snapshot::detached());
    bridge.refresh(0);
    assert_eq!(bridge.choose(true), Err(DETACHED_MESSAGE.to_owned()));
    assert_eq!(bridge.begin_pairing(), Err(DETACHED_MESSAGE.to_owned()));
    assert_eq!(bridge.revoke("d1"), Err(DETACHED_MESSAGE.to_owned()));
    assert_eq!(bridge.snapshot(), Snapshot::detached());
}

#[test]
fn the_choice_is_kept_in_the_apps_settings_file() {
    let dir = std::env::temp_dir().join(format!("notchling-bridge-choice-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let choice = SettingsChoice {
        file: dir.join("settings.json"),
    };
    assert!(!choice.get());
    choice.set(true).unwrap();
    assert!(choice.get());
    assert!(SettingsChoice {
        file: dir.join("settings.json")
    }
    .get());
    choice.set(false).unwrap();
    assert!(!choice.get());
    std::fs::remove_dir_all(&dir).ok();
}
