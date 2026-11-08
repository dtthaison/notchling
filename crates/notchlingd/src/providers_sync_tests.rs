//! A provider sync stuck on a Keychain prompt holds one thread, however often the app asks.

use crate::acting_ops_tests::FakeRoom;
use crate::providers_ops::ProvidersOps;
use crate::switches::{SwitchFlag, ACCOUNTS, ACCOUNTS_OFF};
use notchling_engine::providers::{
    AccountError, AccountProvider, AddProgress, AddStarted, Capabilities, ProviderRegistry,
    ProviderState,
};
use notchling_local_api::ops;
use serde_json::json;
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
