//! Providers that take a while: a switch or an add that outlasts its reply, and a sync stuck on
//! a Keychain prompt, which holds one thread however often the app asks.

use crate::acting_ops_tests::FakeRoom;
use crate::providers_ops::ProvidersOps;
use crate::switches::{SwitchFlag, ACCOUNTS, ACCOUNTS_OFF};
use notchling_engine::providers::{
    AccountError, AccountProvider, AddProgress, AddStarted, Capabilities, ProviderRegistry,
    ProviderState,
};
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// A provider whose sync waits while the gate is closed, and counts the syncs begun.
#[derive(Default)]
struct Stuck {
    closed: Mutex<bool>,
    opened: Condvar,
    begun: AtomicUsize,
}

impl AccountProvider for Stuck {
    fn id(&self) -> notchling_engine::providers::ProviderId {
        "stuck".into()
    }
    fn title(&self) -> &'static str {
        "Stuck"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::USAGE
    }
    fn state(&self) -> ProviderState {
        ProviderState::default()
    }
    fn sync(&self, _force: bool) {
        self.begun.fetch_add(1, Ordering::SeqCst);
        let mut closed = self.closed.lock().unwrap();
        while *closed {
            closed = self.opened.wait(closed).unwrap();
        }
    }
    fn switch_to(&self, _id: &str) -> Result<(), AccountError> {
        Ok(())
    }
    fn begin_add(&self) -> Result<AddStarted, AccountError> {
        Ok(AddStarted::default())
    }
    fn poll_add(&self) -> AddProgress {
        AddProgress::Idle
    }
    fn cancel_add(&self) {}
    fn remove(&self, _id: &str) {}
    fn switch_failure_title(&self) -> &'static str {
        "Couldn't switch Stuck"
    }
}

fn wait_until(what: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !what() {
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_stuck_sync_is_not_started_again_by_every_list_or_refresh() {
    let stuck = Arc::new(Stuck::default());
    *stuck.closed.lock().unwrap() = true;
    let mut registry = ProviderRegistry::new();
    registry.register(stuck.clone());
    let ops = ProvidersOps::new(
        SwitchFlag::in_memory(ACCOUNTS, ACCOUNTS_OFF, true),
        registry,
        Arc::new(FakeRoom::default()),
    );
    ops.set_reply_wait(Duration::from_millis(20));

    for _ in 0..4 {
        let list = ops.call(ops::PROVIDERS_LIST, &json!({})).unwrap();
        assert_eq!(list["syncing"], true);
        let usage = ops
            .call(
                ops::PROVIDERS_USAGE,
                &json!({"provider": "stuck", "refresh": true}),
            )
            .unwrap();
        assert_eq!(usage["refreshing"], true);
    }
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(stuck.begun.load(Ordering::SeqCst), 1, "one sync at a time");

    *stuck.closed.lock().unwrap() = false;
    stuck.opened.notify_all();
    // Once it ends, the next list syncs again.
    wait_until(|| {
        ops.call(ops::PROVIDERS_LIST, &json!({})).unwrap();
        stuck.begun.load(Ordering::SeqCst) >= 2
    });
}

/// A provider whose switch takes a while, to see the reply give up and the event follow.
struct Slow {
    done: Arc<Mutex<bool>>,
}

impl AccountProvider for Slow {
    fn id(&self) -> notchling_engine::providers::ProviderId {
        "slow".into()
    }
    fn title(&self) -> &'static str {
        "Slow"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::ADD_ACCOUNT
    }
    fn state(&self) -> ProviderState {
        ProviderState::default()
    }
    fn sync(&self, _force: bool) {}
    fn switch_to(&self, _id: &str) -> Result<(), AccountError> {
        std::thread::sleep(Duration::from_millis(300));
        *self.done.lock().unwrap() = true;
        Ok(())
    }
    fn begin_add(&self) -> Result<AddStarted, AccountError> {
        std::thread::sleep(Duration::from_millis(300));
        Ok(AddStarted::default())
    }
    fn poll_add(&self) -> AddProgress {
        AddProgress::Done
    }
    fn cancel_add(&self) {}
    fn remove(&self, _id: &str) {}
    fn switch_failure_title(&self) -> &'static str {
        "Couldn't switch Slow"
    }
}

fn slow_rig() -> (ProvidersOps, Arc<FakeRoom>, Arc<Mutex<bool>>) {
    let done = Arc::new(Mutex::new(false));
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(Slow { done: done.clone() }));
    let room = Arc::new(FakeRoom::default());
    let ops = ProvidersOps::new(
        SwitchFlag::in_memory(ACCOUNTS, ACCOUNTS_OFF, true),
        registry,
        room.clone(),
    );
    ops.set_reply_wait(Duration::from_millis(30));
    ops.set_add_timing(Duration::from_millis(10), Duration::from_secs(5));
    (ops, room, done)
}

fn wait_event(room: &FakeRoom, what: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(e) = room
            .named("providers.changed")
            .into_iter()
            .find(|e| what(e))
        {
            return e;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_slow_switch_answers_switching_and_ends_as_an_event_and_one_runs_at_a_time() {
    let (ops, room, done) = slow_rig();
    let args = json!({"provider": "slow", "id": "x"});
    assert_eq!(
        ops.call(ops::PROVIDERS_SWITCH, &args).unwrap(),
        json!({"state": "switching"})
    );
    let second = ops.call(ops::PROVIDERS_SWITCH, &args).unwrap_err();
    assert_eq!(second.code, ErrorCode::Failed);
    assert!(second.message.contains("another account change"));
    let end = wait_event(&room, |e| e["reason"] == "switch" && e["state"] == "done");
    assert_eq!(end["provider"], "slow");
    assert!(*done.lock().unwrap());
    // The slot is free again.
    let again = ops.call(ops::PROVIDERS_SWITCH, &args).unwrap();
    assert!(again["state"] == "switching" || again["state"] == "switched");
}

#[test]
fn a_slow_add_start_answers_starting_and_reports_progress_by_event() {
    let (ops, room, _) = slow_rig();
    let reply = ops
        .call(ops::PROVIDERS_ADD_BEGIN, &json!({"provider": "slow"}))
        .unwrap();
    assert_eq!(reply, json!({"state": "starting"}));
    wait_event(&room, |e| e["reason"] == "add" && e["state"] == "started");
    wait_event(&room, |e| e["reason"] == "add" && e["state"] == "done");
}
