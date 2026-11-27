use crate::cited::Cited;
use crate::consent_testkit::{agent_caller, app_caller};
use crate::cost_asks::CostAsks;
use crate::cost_profile_ops::{guard_after, CostProfileOps};
use crate::install_ops_tests::{ops_for, rig, settings_text, Dir, USER_SETTINGS};
use crate::op_group::OpGroup;
use crate::setup_testkit::{cards, Cards};
use notchling_engine::catalog::Catalog;
use notchling_engine::cost_profile::guard;
use notchling_engine::cost_profile::keys::Probe;
use notchling_engine::cost_profile::Service;
use notchling_local_api::{ops, ApiError};
use serde_json::{json, Value};
use std::sync::Arc;

struct Yes;

impl Probe for Yes {
    fn accepts(&self, _key: &str, _raw: &str) -> bool {
        true
    }
}

struct Rig {
    dir: Dir,
    c: Cards,
    cost: Arc<CostProfileOps>,
    asks: Arc<CostAsks>,
}

fn guarded(tag: &str) -> Rig {
    let dir = Dir::new(&format!("guardcard-{tag}"));
    let mut setup = rig(&dir, Some(USER_SETTINGS));
    setup.guard_data = Some(dir.path("data"));
    std::fs::create_dir_all(dir.path("data")).unwrap();
    let roots = setup.tools.clone();
    let (installing, switch) = ops_for(setup, true);
    let install = Arc::new(installing);
    let service = Service {
        roots,
        data_dir: dir.path("data"),
        catalog: Arc::new(Catalog::built_in()),
        live: Arc::new(Vec::new),
        probe: Arc::new(Yes),
        codex_snapshot: None,
    };
    let cost =
        Arc::new(CostProfileOps::new(service, switch.clone()).with_after(guard_after(install)));
    let c = cards(tag);
    let asks = CostAsks::new(c.consent.clone(), cost.clone(), switch);
    Rig { dir, c, cost, asks }
}

impl Rig {
    fn set_up_by_the_user(&self) {
        self.cost
            .call(&app_caller(), ops::COST_SETUP_APPLY, &json!({}))
            .unwrap();
        assert!(guard::enabled(&self.dir.path("data")));
        assert!(settings_text(&self.dir).contains("hook --guard"));
    }

    fn card(&self, action: &str, args: Value) -> (String, Vec<String>) {
        let reply = self
            .asks
            .call(
                &agent_caller(),
                ops::COST_REQUEST,
                &json!({"action": action, "args": args}),
            )
            .unwrap();
        assert_eq!(reply["state"], "waiting", "{reply}");
        let parts = reply["request"]["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_string())
            .collect();
        (reply["request"]["id"].as_str().unwrap().to_string(), parts)
    }

    fn allow(&self, id: &str, parts: &[String]) -> Result<Value, ApiError> {
        let row = self.c.consent.get_row(id).unwrap();
        let op = row.change["apply"]["op"].as_str().unwrap().to_string();
        let mut args = row.change["apply"]["args"].clone();
        args["consentId"] = json!(id);
        args["parts"] = json!(parts);
        let cost = self.cost.clone();
        let name = op.clone();
        self.asks.apply(&app_caller(), &op, &args, &move |a| {
            cost.call(&app_caller(), &name, a)
        })
    }
}

#[test]
fn an_undo_card_never_takes_the_guard_hook_out_of_settings() {
    let r = guarded("undo");
    r.set_up_by_the_user();
    let (id, parts) = r.card("undo", json!({"tool": "claude"}));
    r.allow(&id, &parts).unwrap();
    assert!(!guard::set_up(&r.dir.path("data")));
    assert!(!guard::enabled(&r.dir.path("data")));
    assert!(settings_text(&r.dir).contains("hook --guard"));
}

#[test]
fn a_profile_card_that_turns_claude_off_never_takes_the_guard_hook_out_of_settings() {
    let r = guarded("profile");
    r.set_up_by_the_user();
    let (id, parts) = r.card("profile", json!({"tools": {"claude": false}}));
    r.allow(&id, &parts).unwrap();
    assert!(!guard::enabled(&r.dir.path("data")));
    assert!(settings_text(&r.dir).contains("hook --guard"));
}

#[test]
fn an_apply_card_never_takes_the_guard_hook_out_of_settings() {
    let r = guarded("apply");
    r.set_up_by_the_user();
    r.cost
        .call(
            &app_caller(),
            ops::COST_PROFILE_SET,
            &json!({"preset": "saver"}),
        )
        .unwrap();
    notchling_engine::settings::Settings::open(&r.dir.path("data/settings.json"))
        .unwrap()
        .set(guard::KEY, Value::from("off"))
        .unwrap();
    let (id, parts) = r.card("apply", json!({}));
    r.allow(&id, &parts).unwrap();
    assert!(!guard::enabled(&r.dir.path("data")));
    assert!(settings_text(&r.dir).contains("hook --guard"));
}

#[test]
fn the_users_own_change_still_takes_the_guard_hook_out() {
    let r = guarded("user");
    r.set_up_by_the_user();
    r.cost
        .call(
            &app_caller(),
            ops::COST_PROFILE_SET,
            &json!({"tools": {"claude": false}}),
        )
        .unwrap();
    assert!(!settings_text(&r.dir).contains("hook --guard"));
}

#[test]
fn a_card_that_sets_the_profile_up_never_puts_the_guard_hook_into_settings() {
    let r = guarded("first");
    r.cost
        .call(
            &app_caller(),
            ops::COST_PROFILE_SET,
            &json!({"preset": "saver"}),
        )
        .unwrap();
    assert!(!guard::set_up(&r.dir.path("data")));
    let before = settings_text(&r.dir);
    assert!(!before.contains("hook --guard"));
    let (id, parts) = r.card("apply", json!({}));
    r.allow(&id, &parts).unwrap();
    assert!(guard::set_up(&r.dir.path("data")));
    assert!(guard::enabled(&r.dir.path("data")));
    assert!(!settings_text(&r.dir).contains("hook --guard"));
}
