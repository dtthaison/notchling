//! `ProviderRegistryTests` and the state tests of `ProviderTests.swift`, on a fake provider.

use super::*;
use std::sync::Mutex;

struct Fake {
    id: &'static str,
    title: &'static str,
    syncs: Mutex<Vec<bool>>,
}

impl Fake {
    fn new(id: &'static str, title: &'static str) -> Arc<Fake> {
        Arc::new(Fake {
            id,
            title,
            syncs: Mutex::new(vec![]),
        })
    }
}

impl AccountProvider for Fake {
    fn id(&self) -> ProviderId {
        ProviderId::new(self.id)
    }
    fn title(&self) -> &'static str {
        self.title
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::ADD_ACCOUNT
    }
    fn state(&self) -> ProviderState {
        ProviderState::default()
    }
    fn sync(&self, force: bool) {
        self.syncs.lock().unwrap().push(force);
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
        "Couldn't switch"
    }
}

#[test]
fn the_registry_keeps_registration_order_and_finds_by_id() {
    let mut registry = ProviderRegistry::new();
    registry.register(Fake::new("a", "A"));
    registry.register(Fake::new("b", "B"));
    let ids: Vec<_> = registry
        .providers()
        .iter()
        .map(|p| p.id().as_str().to_string())
        .collect();
    assert_eq!(ids, vec!["a", "b"]);
    assert_eq!(registry.get("b").unwrap().title(), "B");
    assert!(registry.get("c").is_none());
}

#[test]
fn a_second_provider_with_the_same_id_is_ignored() {
    let mut registry = ProviderRegistry::new();
    registry.register(Fake::new("a", "First"));
    registry.register(Fake::new("a", "Second"));
    assert_eq!(registry.providers().len(), 1);
    assert_eq!(registry.get("a").unwrap().title(), "First");
}

#[test]
fn sync_all_passes_force_to_every_provider() {
    let (a, b) = (Fake::new("a", "A"), Fake::new("b", "B"));
    let mut registry = ProviderRegistry::new();
    registry.register(a.clone());
    registry.register(b.clone());
    registry.sync_all(false);
    registry.sync_all(true);
    assert_eq!(*a.syncs.lock().unwrap(), vec![false, true]);
    assert_eq!(*b.syncs.lock().unwrap(), vec![false, true]);
}

#[test]
fn defaults_are_no_confirmation_no_usage_and_available() {
    let p = Fake::new("a", "A");
    assert!(p.is_available());
    assert!(p.switch_confirmation("x").is_none());
    assert!(p.add_confirmation().is_none());
    assert!(p.usage_report("x").is_none());
}

#[test]
fn state_names_the_active_account() {
    let mut state = ProviderState {
        accounts: vec![
            ProviderAccount::new("1", "a@x.com", None),
            ProviderAccount::new("2", "b@x.com", Some("Pro")),
        ],
        active_id: Some("2".into()),
        ..ProviderState::default()
    };
    assert_eq!(state.active_name().as_deref(), Some("b@x.com"));
    state.active_id = Some("9".into());
    assert_eq!(state.active_name(), None);
    state.unlisted_active_title = Some("Signed in".into());
    assert_eq!(state.active_name().as_deref(), Some("Signed in"));
    state.active_id = None;
    assert_eq!(state.active_name(), None);
}

#[test]
fn capabilities_combine_and_name_themselves() {
    let c = Capabilities::ADD_ACCOUNT
        .with(Capabilities::USAGE)
        .with(Capabilities::AUTO_SWITCH);
    assert!(c.contains(Capabilities::USAGE));
    assert!(!c.contains(Capabilities::RESTARTS_APP));
    assert_eq!(c.names(), vec!["addAccount", "usage", "autoSwitch"]);
    assert_eq!(Capabilities::NONE.names(), Vec::<&str>::new());
}
