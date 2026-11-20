use crate::cost_profile_ops::{CostProfileOps, COST_PROFILE_OPS};
use crate::op_group::OpGroup;
use crate::switches::{SwitchFlag, INSTALL, INSTALL_OFF};
use notchling_engine::catalog::Catalog;
use notchling_engine::cost_profile::keys::Probe;
use notchling_engine::cost_profile::Service;
use notchling_engine::install::ToolRoots;
use notchling_engine::settings::Settings;
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Yes;

impl Probe for Yes {
    fn accepts(&self, _key: &str, _raw: &str) -> bool {
        true
    }
}

struct Rig {
    dir: PathBuf,
    ops: CostProfileOps,
    switch: Arc<SwitchFlag>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

const SETTINGS: &str =
    "{\n  \"env\": {\"API_TOKEN\": \"sk-ant-NEVERSHOWN\"},\n  \"hooks\": {}\n}\n";

fn rig(tag: &str, on: bool) -> Rig {
    build(tag, on, None)
}

fn build(tag: &str, on: bool, after: Option<Arc<dyn Fn() + Send + Sync>>) -> Rig {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "nl-costops-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    let roots = ToolRoots {
        claude: dir.join(".claude"),
        codex: dir.join(".codex"),
        antigravity: dir.join(".gemini/config"),
    };
    fs::create_dir_all(&roots.claude).unwrap();
    fs::create_dir_all(&roots.codex).unwrap();
    fs::write(roots.claude.join("settings.json"), SETTINGS).unwrap();
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    let service = Service {
        roots,
        data_dir: data,
        catalog: Arc::new(Catalog::built_in()),
        live: Arc::new(Vec::new),
        probe: Arc::new(Yes),
        codex_snapshot: None,
    };
    let switch = SwitchFlag::in_memory(INSTALL, INSTALL_OFF, on);
    let mut ops = CostProfileOps::new(service, switch.clone());
    if let Some(after) = after {
        ops = ops.with_after(after);
    }
    Rig { dir, ops, switch }
}

fn caller(tier: Tier) -> Caller {
    Caller {
        tier,
        client: if tier == Tier::App {
            ClientKind::App
        } else {
            ClientKind::Cli
        },
        peer: Some(PathBuf::from("/x/notchling")),
        pid: None,
    }
}

fn call(
    rig: &Rig,
    tier: Tier,
    op: &str,
    args: Value,
) -> Result<Value, notchling_local_api::ApiError> {
    rig.ops.call(&caller(tier), op, &args)
}

fn claude(rig: &Rig, rel: &str) -> PathBuf {
    rig.dir.join(".claude").join(rel)
}

#[test]
fn reads_are_agent_tier_and_writes_are_app_tier() {
    let r = rig("spec", false);
    for op in [
        ops::COST_PROFILE_GET,
        ops::COST_SETUP_PLAN,
        ops::COST_SETUP_CHECK,
    ] {
        assert_eq!(r.ops.spec(op), Some(OpSpec::AGENT), "{op}");
    }
    for op in [
        ops::COST_PROFILE_SET,
        ops::COST_SETUP_APPLY,
        ops::COST_SETUP_UNDO,
    ] {
        assert_eq!(r.ops.spec(op), Some(OpSpec::APP), "{op}");
    }
    assert_eq!(r.ops.spec("cost.report"), None);
    assert_eq!(COST_PROFILE_OPS.len(), 6);
}

#[test]
fn the_profile_starts_balanced_and_an_agent_does_not_see_app_only_data() {
    let r = rig("get", false);
    let agent = call(&r, Tier::Agent, ops::COST_PROFILE_GET, json!({})).unwrap();
    let p = &agent["profile"];
    assert_eq!(p["preset"], "balanced");
    assert_eq!(p["custom"], false);
    assert!(p["table"]["code"]["claude"]["model"].is_string());
    assert_eq!(p["table"]["image"]["claude"], Value::Null);
    assert!(p["presets"]["saver"].is_object() && p["presets"]["quality"].is_object());
    assert!(p.get("guards").is_none() && p.get("imported").is_none());
    let app = call(&r, Tier::App, ops::COST_PROFILE_GET, json!({})).unwrap();
    assert!(app["profile"].get("guards").is_some());
}

#[test]
fn set_checks_every_part_and_persists_the_profile() {
    let r = rig("set", false);
    let set = |args: Value| call(&r, Tier::App, ops::COST_PROFILE_SET, args);
    for bad in [
        json!({"preset": "free"}),
        json!({"picks": {"code": "opus high"}}),
        json!({"picks": {"code.claude": "opus"}}),
        json!({"picks": {"nope.claude": "opus high"}}),
        json!({"habits": {"maxTurns": 1}}),
        json!({"habits": {"nonsense": 1}}),
        json!({"tools": {"claude": "yes"}}),
        json!({"guards": {"Poll": "block"}}),
        json!({"extra": 1}),
    ] {
        let e = set(bad.clone()).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{bad}");
    }
    let done = set(json!({
        "preset": "saver",
        "picks": {"code.claude": "opus medium"},
        "habits": {"parallelCap": 3},
        "guards": {"poll": "block"},
        "tools": {"agy": false},
    }))
    .unwrap();
    let p = &done["profile"];
    assert_eq!(p["preset"], "saver");
    assert_eq!(p["custom"], true);
    assert_eq!(p["habits"]["parallelCap"], 3);
    assert_eq!(p["tools"]["agy"], false);
    assert_eq!(p["guards"]["poll"], "block");
    assert_eq!(p["table"]["code"]["claude"]["effort"], "medium");
    let again = call(&r, Tier::Agent, ops::COST_PROFILE_GET, json!({})).unwrap();
    assert_eq!(again["profile"]["preset"], "saver");
    assert!(again["profile"].get("guards").is_none());
    let store = Settings::open(&r.dir.join("data/settings.json")).unwrap();
    assert_eq!(store.get_str("costPreset").as_deref(), Some("saver"));
    assert_eq!(store.get_bool("costClaude"), Some(true));
    assert_eq!(store.get_bool("costAgy"), Some(false));
    assert_eq!(
        store.get_str("costPicks").as_deref(),
        Some("code.claude=opus medium")
    );
    assert_eq!(
        store.get("costProfileApp").unwrap()["guards"]["poll"],
        "block"
    );
    let reset = set(json!({"reset": true, "preset": "quality"})).unwrap();
    assert_eq!(reset["profile"]["custom"], false);
}

#[test]
fn a_plan_writes_nothing_and_hides_the_settings_json_text_from_an_agent() {
    let r = rig("plan", false);
    let before = fs::read_to_string(claude(&r, "settings.json")).unwrap();
    let agent = call(&r, Tier::Agent, ops::COST_SETUP_PLAN, json!({})).unwrap();
    assert_eq!(agent["on"], false);
    assert!(!claude(&r, "rules/notchling-cost.md").exists());
    assert_eq!(
        fs::read_to_string(claude(&r, "settings.json")).unwrap(),
        before
    );
    let actions = agent["plan"]["actions"].as_array().unwrap();
    let settings = actions
        .iter()
        .find(|a| a["id"] == "claude.settings")
        .unwrap();
    assert_eq!(settings["kind"], "update");
    assert_eq!(settings["diff"], "");
    assert!(settings["keys"]
        .as_array()
        .unwrap()
        .iter()
        .any(|k| k["key"] == "bashOutputMaxChars"));
    assert!(!agent.to_string().contains("NEVERSHOWN"));
    let rule = actions.iter().find(|a| a["id"] == "claude.rule").unwrap();
    assert!(rule["diff"].as_str().unwrap().contains("+"));
    let app = call(&r, Tier::App, ops::COST_SETUP_PLAN, json!({})).unwrap();
    let shown = app["plan"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "claude.settings")
        .unwrap();
    assert!(shown["diff"]
        .as_str()
        .unwrap()
        .contains("bashOutputMaxChars"));
    let e = call(
        &r,
        Tier::App,
        ops::COST_SETUP_PLAN,
        json!({"consent": ["everything"]}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
}

#[test]
fn apply_and_undo_do_nothing_while_the_install_switch_is_off() {
    let r = rig("off", false);
    let reply = call(&r, Tier::App, ops::COST_SETUP_APPLY, json!({})).unwrap();
    assert_eq!(reply["on"], false);
    assert_eq!(reply["applied"], false);
    assert_eq!(reply["message"], INSTALL_OFF);
    assert!(reply["plan"]["changes"].as_bool().unwrap());
    assert!(!claude(&r, "rules/notchling-cost.md").exists());
    let undo = call(
        &r,
        Tier::App,
        ops::COST_SETUP_UNDO,
        json!({"tool": "claude"}),
    )
    .unwrap();
    assert_eq!(undo["on"], false);
    let check = call(
        &r,
        Tier::App,
        ops::COST_SETUP_CHECK,
        json!({"resolve": {"id": "claude.rule", "choice": "keep"}}),
    )
    .unwrap();
    assert_eq!(check["on"], false);
}

#[test]
fn apply_check_and_undo_round_trip_with_the_switch_on() {
    let r = rig("round", true);
    let before = fs::read_to_string(claude(&r, "settings.json")).unwrap();
    let done = call(&r, Tier::App, ops::COST_SETUP_APPLY, json!({})).unwrap();
    assert_eq!(done["applied"], true);
    assert_eq!(done["failed"], false);
    assert!(claude(&r, "rules/notchling-cost.md").exists());
    assert!(claude(&r, "agents/notchling-builder.md").exists());
    assert!(fs::read_to_string(claude(&r, "settings.json"))
        .unwrap()
        .contains("bashOutputMaxChars"));
    assert!(r.dir.join("data/daemon/cost-setup.json").exists());
    let again = call(&r, Tier::Agent, ops::COST_SETUP_PLAN, json!({})).unwrap();
    assert_eq!(again["plan"]["changes"], false);
    let check = call(&r, Tier::Agent, ops::COST_SETUP_CHECK, json!({})).unwrap();
    assert_eq!(check["drift"], false);
    assert!(check["rows"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["id"] == "claude.rule" && row["state"] == "in_step"));

    fs::write(claude(&r, "rules/notchling-cost.md"), "mine\n").unwrap();
    let drifted = call(&r, Tier::Agent, ops::COST_SETUP_CHECK, json!({})).unwrap();
    assert_eq!(drifted["drift"], true);
    let refused = call(
        &r,
        Tier::Agent,
        ops::COST_SETUP_CHECK,
        json!({"resolve": {"id": "claude.rule", "choice": "restore"}}),
    )
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Forbidden);
    assert_eq!(
        fs::read_to_string(claude(&r, "rules/notchling-cost.md")).unwrap(),
        "mine\n"
    );
    let restored = call(
        &r,
        Tier::App,
        ops::COST_SETUP_CHECK,
        json!({"resolve": {"id": "claude.rule", "choice": "restore"}}),
    )
    .unwrap();
    assert_eq!(restored["drift"], false);
    assert!(fs::read_to_string(claude(&r, "rules/notchling-cost.md"))
        .unwrap()
        .contains("notchling-cost v1"));

    let undone = call(
        &r,
        Tier::App,
        ops::COST_SETUP_UNDO,
        json!({"tool": "claude"}),
    )
    .unwrap();
    assert_eq!(undone["conflicts"], 0);
    assert!(!claude(&r, "rules/notchling-cost.md").exists());
    assert!(!claude(&r, "agents/notchling-builder.md").exists());
    let back: Value =
        serde_json::from_str(&fs::read_to_string(claude(&r, "settings.json")).unwrap()).unwrap();
    assert_eq!(back, serde_json::from_str::<Value>(&before).unwrap());
    let e = call(
        &r,
        Tier::App,
        ops::COST_SETUP_UNDO,
        json!({"tool": "gemini"}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    r.switch.check().unwrap();
}

#[test]
fn a_consent_the_person_did_not_give_leaves_that_part_an_offer() {
    let r = rig("consent", true);
    fs::create_dir_all(claude(&r, "rules")).unwrap();
    fs::write(
        claude(&r, "rules/model-choice.md"),
        "# Choosing a subagent model\n\n- sonnet high for code\n",
    )
    .unwrap();
    let plan = call(&r, Tier::App, ops::COST_SETUP_PLAN, json!({})).unwrap();
    let replaced = plan["plan"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "claude.rules.replaced.model-choice.md")
        .unwrap();
    assert_eq!(replaced["kind"], "offer");
    assert_eq!(replaced["needs"], "rules");
    call(&r, Tier::App, ops::COST_SETUP_APPLY, json!({})).unwrap();
    assert!(claude(&r, "rules/model-choice.md").exists());
    call(
        &r,
        Tier::App,
        ops::COST_SETUP_APPLY,
        json!({"consent": ["rules"]}),
    )
    .unwrap();
    assert!(!claude(&r, "rules/model-choice.md").exists());
    let undo = call(
        &r,
        Tier::App,
        ops::COST_SETUP_UNDO,
        json!({"tool": "claude"}),
    )
    .unwrap();
    assert_eq!(undo["conflicts"], 0);
    assert!(fs::read_to_string(claude(&r, "rules/model-choice.md"))
        .unwrap()
        .contains("sonnet high for code"));
}

fn counted() -> (Arc<AtomicUsize>, Arc<dyn Fn() + Send + Sync>) {
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    (
        count,
        Arc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
        }),
    )
}

fn policy_file(r: &Rig) -> PathBuf {
    r.dir.join("data/guard/policy.json")
}

#[test]
fn the_profile_tells_the_guard_level_and_whether_it_is_on() {
    let r = rig("guard-get", false);
    let reply = call(&r, Tier::Agent, ops::COST_PROFILE_GET, json!({})).unwrap();
    assert_eq!(
        reply["profile"]["guard"],
        json!({"level": "log", "set_up": false, "on": false})
    );
}

#[test]
fn set_up_turns_the_guard_policy_on_and_undo_turns_it_off() {
    let (count, after) = counted();
    let r = build("guard-apply", true, Some(after));
    assert!(!policy_file(&r).exists());
    call(&r, Tier::App, ops::COST_SETUP_APPLY, json!({})).unwrap();
    assert!(policy_file(&r).exists());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let text: Value = serde_json::from_str(&fs::read_to_string(policy_file(&r)).unwrap()).unwrap();
    assert_eq!(text["level"], "log");
    assert_eq!(text["cap"], 5);
    let got = call(&r, Tier::App, ops::COST_PROFILE_GET, json!({})).unwrap();
    assert_eq!(got["profile"]["guard"]["on"], true);
    call(
        &r,
        Tier::App,
        ops::COST_SETUP_UNDO,
        json!({"tool": "claude"}),
    )
    .unwrap();
    assert!(!policy_file(&r).exists());
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn a_set_up_that_changed_nothing_leaves_the_guard_alone() {
    let (count, after) = counted();
    let r = build("guard-off-switch", false, Some(after));
    call(&r, Tier::App, ops::COST_SETUP_APPLY, json!({})).unwrap();
    assert!(!policy_file(&r).exists());
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn the_guard_level_is_set_by_the_app_and_off_stops_the_policy() {
    let (count, after) = counted();
    let r = build("guard-set", true, Some(after));
    call(&r, Tier::App, ops::COST_SETUP_APPLY, json!({})).unwrap();
    let off = call(
        &r,
        Tier::App,
        ops::COST_PROFILE_SET,
        json!({"guard": "off"}),
    )
    .unwrap();
    assert_eq!(off["profile"]["guard"]["level"], "off");
    assert_eq!(off["profile"]["guard"]["on"], false);
    assert!(!policy_file(&r).exists());
    let store = Settings::open(&r.dir.join("data/settings.json")).unwrap();
    assert_eq!(store.get_str("costGuard").as_deref(), Some("off"));
    let on = call(
        &r,
        Tier::App,
        ops::COST_PROFILE_SET,
        json!({"guard": "log"}),
    )
    .unwrap();
    assert_eq!(on["profile"]["guard"]["on"], true);
    assert!(policy_file(&r).exists());
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[test]
fn a_bad_guard_level_is_refused_and_an_agent_cannot_set_one() {
    let r = rig("guard-bad", true);
    for bad in [json!("loud"), json!(5), json!(null), json!("block")] {
        let e = call(&r, Tier::App, ops::COST_PROFILE_SET, json!({"guard": bad})).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest);
    }
    assert_eq!(r.ops.spec(ops::COST_PROFILE_SET), Some(OpSpec::APP));
}

#[test]
fn the_builder_cap_in_the_policy_follows_the_profile() {
    let r = build("guard-cap", true, None);
    call(&r, Tier::App, ops::COST_SETUP_APPLY, json!({})).unwrap();
    call(
        &r,
        Tier::App,
        ops::COST_PROFILE_SET,
        json!({"habits": {"parallelCap": 3}}),
    )
    .unwrap();
    let text: Value = serde_json::from_str(&fs::read_to_string(policy_file(&r)).unwrap()).unwrap();
    assert_eq!(text["cap"], 3);
}
