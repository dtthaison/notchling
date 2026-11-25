use super::wire;
use serde_json::json;

#[test]
fn a_preset_choice_clears_the_edits() {
    let r = wire::preset("saver");
    assert_eq!(r.op, "cost.profile.set");
    assert_eq!(r.args, json!({"preset": "saver", "reset": true}));
}

#[test]
fn a_pick_is_role_dot_tool_with_model_and_effort() {
    let r = wire::pick("judge", "claude", "opus xhigh");
    assert_eq!(r.args, json!({"picks": {"judge.claude": "opus xhigh"}}));
    let r = wire::clear_pick("image", "agy");
    assert_eq!(r.args, json!({"picks": {"image.agy": null}}));
}

#[test]
fn a_habit_edit_sends_only_that_number() {
    let r = wire::habit("parallelCap", 7);
    assert_eq!(r.op, "cost.profile.set");
    assert_eq!(r.args, json!({"habits": {"parallelCap": 7}}));
}

#[test]
fn setup_calls_carry_the_consent_names() {
    let consent = vec!["orchestrator_new".to_owned()];
    assert_eq!(wire::plan(&consent).op, "cost.setup.plan");
    assert_eq!(
        wire::apply(&consent).args,
        json!({"consent": ["orchestrator_new"]})
    );
    assert_eq!(wire::undo("codex").args, json!({"tool": "codex"}));
}

#[test]
fn resolving_goes_through_check() {
    let r = wire::resolve("claude.rule", "keep");
    assert_eq!(r.op, "cost.setup.check");
    assert_eq!(
        r.args,
        json!({"resolve": {"id": "claude.rule", "choice": "keep"}})
    );
}
