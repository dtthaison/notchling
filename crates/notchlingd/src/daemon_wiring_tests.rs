use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::switches::{ACCOUNTS, VAULT};
use notchling_local_api::{ops, ClientKind};
use serde_json::json;

struct Root(PathBuf);

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wired(tag: &str) -> (Root, Layout, Wired) {
    let root = std::env::temp_dir().join(format!("nl-wiring-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
    assert!(layout.fixture);
    let sessions = Arc::new(Mutex::new(json!([])));
    let wired = Wired::build(&layout, sessions, Arc::new(FakeRoom::default()), (0, 0));
    (Root(root), layout, wired)
}

fn app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

#[test]
fn every_switch_starts_off_and_every_watcher_is_fed() {
    let (_root, _layout, w) = wired("off");
    for flag in w.switches.all() {
        assert!(!flag.is_on(), "{}", flag.name());
    }
    let status = w.limits.call("limits.status", &json!({})).unwrap();
    assert_eq!(status["accounts_on"], false);
    assert_eq!(status["watching"], false);
    for provider in status["providers"].as_array().unwrap() {
        assert_eq!(provider["registered"], true, "{provider}");
    }
    assert!(!w.limits.tick_at(1_800_000_000.0));
}

#[test]
fn the_acting_ops_check_models_against_the_daemons_own_catalog() {
    let (_root, _layout, w) = wired("catalog");
    assert!(
        std::ptr::eq(w.acting.catalog(), &*w.catalog),
        "session.new --model must read the catalog catalog.get serves"
    );
}

#[test]
fn with_accounts_on_a_round_runs_over_the_fixture_stores_and_reaches_no_login() {
    let (_root, _layout, w) = wired("on");
    assert_eq!(w.accounts_switch().name(), ACCOUNTS);
    w.accounts_switch().set(true);
    assert!(w.limits.tick_at(1_800_000_000.0));
    let status = w.limits.call("limits.status", &json!({})).unwrap();
    assert_eq!(status["watching"], true);
    for provider in status["providers"].as_array().unwrap() {
        assert_eq!(provider["active"], Value::Null, "{provider}");
        assert_eq!(provider["last_switch"], Value::Null, "{provider}");
    }
    w.accounts_switch().set(false);
    assert!(!w.limits.tick_at(1_800_000_400.0));
}

#[test]
fn the_server_serves_every_area_and_the_vault_skill_is_offered_to_install() {
    let (_root, layout, w) = wired("ops");
    let ops = w.ops(
        &layout,
        Arc::new(Mutex::new(json!([]))),
        FirstScan::open(),
        Duration::ZERO,
    );
    for op in [
        ops::SETTINGS_SWITCHES,
        ops::ACCOUNTS_LIST,
        ops::PROVIDERS_LIST,
        "limits.status",
        "cost.summary",
        ops::CATALOG_GET,
        ops::VAULT_LIST,
        ops::INSTALL_PLAN,
    ] {
        assert!(ops.spec(op).is_some(), "{op}");
    }
    for topic in ["accounts", "providers", "limits", "vault"] {
        assert!(ops.topics().contains(&topic), "{topic}");
        assert_eq!(ops.topic_tier(topic), Tier::App, "{topic}");
    }
    assert_eq!(w.switches.vault.name(), VAULT);
    let plan = ops
        .call(
            &app(),
            ops::INSTALL_PLAN,
            &json!({"what": ["notchling-vault"]}),
        )
        .unwrap();
    assert!(plan.to_string().contains("notchling-vault"), "{plan}");
    assert!(w.vault.skill_names().is_empty());
    w.close();
}
