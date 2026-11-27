use super::*;
use crate::consent_testkit::{agent_caller, app_caller};
use crate::cost_profile_ops::CostProfileOps;
use crate::setup_testkit::{cards, reason_of, snapshot, Cards};
use notchling_engine::catalog::Catalog;
use notchling_engine::cost_profile::keys::Probe;
use notchling_engine::cost_profile::Service;
use notchling_engine::install::ToolRoots;
use notchling_local_api::ErrorCode;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Yes;

impl Probe for Yes {
    fn accepts(&self, _key: &str, _raw: &str) -> bool {
        true
    }
}

const SETTINGS: &str =
    "{\n  \"env\": {\"API_TOKEN\": \"sk-ant-NEVERSHOWN\"},\n  \"hooks\": {}\n}\n";

struct Rig {
    dir: PathBuf,
    c: Cards,
    cost: Arc<CostProfileOps>,
    asks: Arc<CostAsks>,
    switch: Arc<SwitchFlag>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn rig(tag: &str, on: bool) -> Rig {
    rig_probing(tag, on, Arc::new(Yes))
}

struct Counting(Arc<AtomicUsize>);

impl Probe for Counting {
    fn accepts(&self, _key: &str, _raw: &str) -> bool {
        self.0.fetch_add(1, Ordering::SeqCst);
        true
    }
}

fn rig_probing(tag: &str, on: bool, probe: Arc<dyn Probe + Send + Sync>) -> Rig {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "nl-costasks-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let roots = ToolRoots {
        claude: dir.join(".claude"),
        codex: dir.join(".codex"),
        antigravity: dir.join(".gemini/config"),
    };
    std::fs::create_dir_all(&roots.claude).unwrap();
    std::fs::create_dir_all(&roots.codex).unwrap();
    std::fs::write(roots.claude.join("settings.json"), SETTINGS).unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    let service = Service {
        roots,
        data_dir: dir.join("data"),
        catalog: Arc::new(Catalog::built_in()),
        live: Arc::new(Vec::new),
        probe,
        codex_snapshot: None,
    };
    let switch = SwitchFlag::in_memory(crate::switches::INSTALL, INSTALL_OFF, on);
    let cost = Arc::new(CostProfileOps::new(service, switch.clone()));
    let c = cards(tag);
    let asks = CostAsks::new(c.consent.clone(), cost.clone(), switch.clone());
    Rig {
        dir,
        c,
        cost,
        asks,
        switch,
    }
}

impl Rig {
    fn ask(&self, action: &str, args: Value) -> Result<Value, ApiError> {
        self.asks.call(
            &agent_caller(),
            ops::COST_REQUEST,
            &json!({"action": action, "args": args}),
        )
    }

    fn card(&self, action: &str, args: Value) -> (String, Vec<Value>) {
        let reply = self.ask(action, args).unwrap();
        assert_eq!(reply["state"], "waiting", "{reply}");
        (
            reply["request"]["id"].as_str().unwrap().to_string(),
            reply["request"]["parts"].as_array().unwrap().clone(),
        )
    }

    fn apply(&self, id: &str, parts: &[&str]) -> Result<Value, ApiError> {
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

    fn claude(&self, rel: &str) -> PathBuf {
        self.dir.join(".claude").join(rel)
    }

    fn files(&self) -> crate::setup_testkit::Snapshot {
        snapshot(&self.dir)
    }
}

fn id_ending(parts: &[Value], tail: &str) -> String {
    parts
        .iter()
        .find(|p| p["path"].as_str().unwrap().ends_with(tail))
        .unwrap_or_else(|| panic!("no part ending {tail}: {parts:?}"))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn state_of(out: &Value, id: &str) -> String {
    out["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap_or_else(|| panic!("no state for {id}: {out}"))["state"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn cost_request_is_an_agent_acts_op_and_only_it() {
    let r = rig("spec", true);
    assert_eq!(r.asks.spec(ops::COST_REQUEST), Some(OpSpec::AGENT_ACTS));
    assert_eq!(r.asks.spec(ops::COST_SETUP_APPLY), None);
    assert!(r.asks.spec("cost.other").is_none());
}

#[test]
fn a_guard_or_guards_change_is_never_asked_and_writes_nothing() {
    let r = rig("never", true);
    let before = r.files();
    for args in [
        json!({"guard": "off"}),
        json!({"guards": {"x": true}}),
        json!({"preset": "saver", "guard": "log"}),
    ] {
        let e = r.ask("profile", args).unwrap_err();
        assert_eq!(reason_of(&e), "never");
    }
    assert_eq!(r.files(), before);
}

#[test]
fn a_consent_with_nothing_to_change_writes_nothing_and_opens_no_profile_card() {
    let r = rig("consent-alone", true);
    let before = r.files();
    let e = r.ask("profile", json!({"consent": ["rules"]})).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    let e = r.ask("profile", json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    let (_, parts) = r.card("apply", json!({"consent": ["rules"]}));
    assert!(!parts.is_empty());
    assert_eq!(r.files(), before);
    assert!(!r.claude("rules/notchling-cost.md").exists());
}

#[test]
fn an_unticked_file_is_not_written() {
    let r = rig("unticked", true);
    let (id, parts) = r.card("apply", json!({}));
    let rule = id_ending(&parts, "notchling-cost.md");
    let out = r.apply(&id, &[rule.as_str()]).unwrap();
    assert_eq!(state_of(&out, &rule), "written");
    assert!(r.claude("rules/notchling-cost.md").exists());
    assert!(!r.claude("agents/notchling-builder.md").exists());
    assert_eq!(r.c.consent.get_row(&id).unwrap().state.as_str(), "applied");
}

#[test]
fn a_file_changed_after_allow_is_reported_stale_and_not_written() {
    let r = rig("stale", true);
    let (id, parts) = r.card("apply", json!({}));
    let rule = id_ending(&parts, "notchling-cost.md");
    let builder = id_ending(&parts, "notchling-builder.md");
    std::fs::create_dir_all(r.claude("rules")).unwrap();
    std::fs::write(r.claude("rules/notchling-cost.md"), "mine\n").unwrap();
    let out = r.apply(&id, &[rule.as_str(), builder.as_str()]).unwrap();
    assert_eq!(state_of(&out, &rule), "stale");
    assert_eq!(state_of(&out, &builder), "written");
    assert_eq!(
        std::fs::read_to_string(r.claude("rules/notchling-cost.md")).unwrap(),
        "mine\n"
    );
    assert!(r.claude("agents/notchling-builder.md").exists());
}

#[test]
fn an_apply_naming_a_part_outside_the_card_is_refused() {
    let r = rig("outside", true);
    let (id, _) = r.card("apply", json!({}));
    let before = r.files();
    for named in [vec!["nope"], vec![]] {
        let e = r.apply(&id, &named).unwrap_err();
        assert_eq!(reason_of(&e), "bad_value");
    }
    assert_eq!(r.files(), before);
}

#[test]
fn an_undo_restores_only_the_kept_files() {
    let r = rig("undo", true);
    r.cost
        .call(&app_caller(), ops::COST_SETUP_APPLY, &json!({}))
        .unwrap();
    assert!(r.claude("rules/notchling-cost.md").exists());
    assert!(r.claude("agents/notchling-builder.md").exists());
    let (id, parts) = r.card("undo", json!({"tool": "claude"}));
    let rule = id_ending(&parts, "notchling-cost.md");
    let out = r.apply(&id, &[rule.as_str()]).unwrap();
    assert_eq!(out["parts"].as_array().unwrap().len(), 1);
    assert!(!r.claude("rules/notchling-cost.md").exists());
    assert!(r.claude("agents/notchling-builder.md").exists());
}

#[test]
fn an_undo_of_a_file_changed_since_the_card_is_stale() {
    let r = rig("undo-stale", true);
    r.cost
        .call(&app_caller(), ops::COST_SETUP_APPLY, &json!({}))
        .unwrap();
    let (id, parts) = r.card("undo", json!({"tool": "claude"}));
    let rule = id_ending(&parts, "notchling-cost.md");
    std::fs::write(r.claude("rules/notchling-cost.md"), "mine\n").unwrap();
    let out = r.apply(&id, &[rule.as_str()]).unwrap();
    assert_eq!(state_of(&out, &rule), "stale");
    assert_eq!(
        std::fs::read_to_string(r.claude("rules/notchling-cost.md")).unwrap(),
        "mine\n"
    );
}

#[test]
fn apply_and_undo_need_the_install_switch_but_a_profile_change_does_not() {
    let r = rig("off", false);
    for (action, args) in [("apply", json!({})), ("undo", json!({"tool": "claude"}))] {
        let e = r.ask(action, args).unwrap_err();
        assert_eq!(reason_of(&e), "switch_off");
        assert_eq!(
            e.data.as_ref().unwrap()["ask"]["command"],
            "notchling switch on install"
        );
    }
    let (_, parts) = r.card("profile", json!({"preset": "saver"}));
    assert_eq!(parts[0]["id"], "profile");
    assert_eq!(parts[0]["tool"], "notchling");
    assert!(parts[0]["fields"].as_array().is_some_and(|f| !f.is_empty()));
    r.switch.set(true);
    let (id, _) = r.card("apply", json!({}));
    r.switch.set(false);
    let e = r.apply(&id, &["claude.rule"]).unwrap_err();
    assert_eq!(reason_of(&e), "switch_off");
    assert!(!r.claude("rules/notchling-cost.md").exists());
}

#[test]
fn a_profile_change_is_written_once_with_the_hash_it_was_asked_from() {
    let r = rig("profile", true);
    let (id, _) = r.card("profile", json!({"preset": "saver"}));
    let row = r.c.consent.get_row(&id).unwrap();
    assert_eq!(row.change["apply"]["op"], ops::COST_PROFILE_SET);
    assert!(row.change["apply"]["args"]["expect"].is_string());
    r.apply(&id, &["profile"]).unwrap();
    let got = r
        .cost
        .call(&agent_caller(), ops::COST_PROFILE_GET, &json!({}))
        .unwrap();
    assert_eq!(got["profile"]["preset"], "saver");
}

#[test]
fn a_profile_changed_after_the_card_is_stale_and_not_written() {
    let r = rig("profile-stale", true);
    let (id, _) = r.card("profile", json!({"preset": "saver"}));
    r.cost
        .call(
            &app_caller(),
            ops::COST_PROFILE_SET,
            &json!({"preset": "quality"}),
        )
        .unwrap();
    let e = r.apply(&id, &["profile"]).unwrap_err();
    assert_eq!(reason_of(&e), "stale");
    let got = r
        .cost
        .call(&agent_caller(), ops::COST_PROFILE_GET, &json!({}))
        .unwrap();
    assert_eq!(got["profile"]["preset"], "quality");
}

fn waiting_cli_sees(r: &Rig, id: &str) -> (String, String) {
    let row = r.c.consent.get_row(id).unwrap();
    let state = row.state.as_str().to_string();
    let failure = crate::consent_cli::failure_of(&json!({"id": id, "state": state}), "consent.get");
    (state, failure.reason)
}

#[test]
fn an_apply_that_is_stale_withdraws_the_card_and_the_waiting_cli_sees_withdrawn() {
    let r = rig("stale-withdrawn", true);
    let (id, _) = r.card("profile", json!({"preset": "saver"}));
    r.cost
        .call(
            &app_caller(),
            ops::COST_PROFILE_SET,
            &json!({"preset": "quality"}),
        )
        .unwrap();
    let e = r.apply(&id, &["profile"]).unwrap_err();
    assert_eq!(reason_of(&e), "stale");
    assert_eq!(
        waiting_cli_sees(&r, &id),
        ("withdrawn".to_string(), "withdrawn".to_string())
    );
}

#[test]
fn a_part_that_is_skipped_by_apply_time_is_stale_not_unchanged_with_no_expect_after() {
    let r = rig("skip-stale", true);
    std::fs::write(r.dir.join(".codex/AGENTS.override.md"), "mine\n").unwrap();
    let out = r
        .cost
        .call(
            &app_caller(),
            ops::COST_SETUP_APPLY,
            &json!({"parts": {"codex.agents_md": null}}),
        )
        .unwrap();
    assert_eq!(out["parts"][0]["id"], "codex.agents_md", "{out}");
    assert_eq!(out["parts"][0]["state"], "stale", "{out}");
    assert!(!r.dir.join(".codex/AGENTS.md").exists());
}

#[test]
fn an_apply_with_the_shown_after_hashes_plans_once_so_codex_is_probed_once_per_key() {
    let probes = Arc::new(AtomicUsize::new(0));
    let r = rig_probing("plan-once", true, Arc::new(Counting(probes.clone())));
    std::fs::write(r.dir.join(".codex/config.toml"), "").unwrap();
    let (id, parts) = r.card("apply", json!({"consent": ["codex_config"]}));
    let per_plan = probes.swap(0, Ordering::SeqCst);
    assert!(per_plan > 0, "the card never reached the probe");
    let config = id_ending(&parts, "config.toml");
    let out = r.apply(&id, &[config.as_str()]).unwrap();
    assert_eq!(state_of(&out, &config), "written", "{out}");
    assert_eq!(probes.load(Ordering::SeqCst), per_plan);
}

#[test]
fn a_call_carrying_a_consent_id_is_claimed_whatever_the_id_and_never_reaches_the_raw_op() {
    let r = rig("claims-all", true);
    for op in [
        ops::COST_SETUP_APPLY,
        ops::COST_SETUP_UNDO,
        ops::COST_PROFILE_SET,
    ] {
        for id in [json!("gone"), json!(7), json!(null), json!(true)] {
            assert!(r.asks.claims(op, &json!({"consentId": id})), "{op} {id}");
        }
        assert!(!r.asks.claims(op, &json!({"preset": "saver"})), "{op}");
    }
    let ran = Arc::new(AtomicUsize::new(0));
    let count = ran.clone();
    let run = move |_: &Value| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(json!({}))
    };
    let e = r
        .asks
        .apply(
            &app_caller(),
            ops::COST_PROFILE_SET,
            &json!({"consentId": "gone", "preset": "saver"}),
            &run,
        )
        .unwrap_err();
    assert_eq!(reason_of(&e), "needs_consent");
    let e = r
        .asks
        .apply(
            &app_caller(),
            ops::COST_SETUP_APPLY,
            &json!({"consentId": 7}),
            &run,
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    assert_eq!(ran.load(Ordering::SeqCst), 0);
}

#[test]
fn a_file_rendered_from_a_profile_that_changed_since_the_card_is_stale_and_not_written() {
    let r = rig("after", true);
    let (id, parts) = r.card("apply", json!({}));
    let rule = id_ending(&parts, "notchling-cost.md");
    r.cost
        .call(
            &app_caller(),
            ops::COST_PROFILE_SET,
            &json!({"preset": "saver"}),
        )
        .unwrap();
    let out = r.apply(&id, &[rule.as_str()]).unwrap();
    assert_eq!(state_of(&out, &rule), "stale");
    assert!(!r.claude("rules/notchling-cost.md").exists());
}
