use super::picks::{
    below_floor, choose_model, effort_allowed, model_allowed, pick_text, Options, NO_MODEL,
};
use super::profile::Floor;
use super::renders::options;
use serde_json::json;

fn floor(text: &str) -> Floor {
    Floor::parse(text).unwrap()
}

#[test]
fn claude_models_are_the_families_the_catalog_lists() {
    let o = options();
    let ids: Vec<&str> = o.tools[0].iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["haiku", "sonnet", "opus", "fable"]);
    let opus = &o.tools[0][2];
    assert_eq!(opus.efforts, ["low", "medium", "high", "xhigh", "max"]);
}

#[test]
fn a_family_the_catalog_lacks_is_not_offered() {
    let claude = json!({"models": [{"id": "claude-sonnet-5-5", "efforts": ["low", "high"]}]});
    let o = Options::from_replies(Some(&claude), None, None);
    assert_eq!(o.tools[0].len(), 1);
    assert_eq!(o.tools[0][0].id, "sonnet");
    assert_eq!(o.tools[0][0].efforts, ["low", "high"]);
}

#[test]
fn without_a_catalog_claude_offers_the_known_families() {
    let o = Options::from_replies(None, None, None);
    assert_eq!(o.tools[0].len(), 4);
    assert!(o.tools[1].is_empty());
}

#[test]
fn a_pick_below_the_floor_is_off() {
    let f = floor("opus high");
    assert!(below_floor(0, Some(&f), "sonnet", "xhigh"));
    assert!(below_floor(0, Some(&f), "haiku", "max"));
    assert!(below_floor(0, Some(&f), "opus", "medium"));
    assert!(!below_floor(0, Some(&f), "opus", "high"));
    assert!(!below_floor(0, Some(&f), "opus", "xhigh"));
    assert!(!below_floor(0, Some(&f), "fable", "low"));
}

#[test]
fn floors_bind_only_the_claude_column() {
    let f = floor("opus high");
    assert!(!below_floor(1, Some(&f), "gpt-6-luna", "low"));
    assert!(!below_floor(2, Some(&f), "gemini-4-hydrogen", "low"));
    assert!(!below_floor(0, None, "haiku", "low"));
}

#[test]
fn a_model_is_off_when_none_of_its_efforts_clear_the_floor() {
    let o = options();
    let f = floor("opus high");
    let allowed: Vec<bool> = o.tools[0]
        .iter()
        .map(|m| model_allowed(0, Some(&f), m))
        .collect();
    assert_eq!(allowed, [false, false, true, true]);
}

#[test]
fn efforts_under_the_floor_are_off_for_the_floor_model_only() {
    let f = floor("opus xhigh");
    assert!(!effort_allowed(0, Some(&f), "opus", "high"));
    assert!(effort_allowed(0, Some(&f), "opus", "xhigh"));
    assert!(effort_allowed(0, Some(&f), "fable", "medium"));
}

#[test]
fn choosing_a_model_keeps_the_effort_when_it_is_allowed() {
    let o = options();
    let opus = &o.tools[0][2];
    let f = floor("opus high");
    assert_eq!(
        choose_model(0, Some(&f), opus, "xhigh").as_deref(),
        Some("opus xhigh")
    );
}

#[test]
fn choosing_a_model_lifts_the_effort_to_the_floor() {
    let o = options();
    let opus = &o.tools[0][2];
    let f = floor("opus high");
    assert_eq!(
        choose_model(0, Some(&f), opus, "low").as_deref(),
        Some("opus high")
    );
}

#[test]
fn choosing_a_model_with_no_allowed_effort_gives_nothing() {
    let o = options();
    let haiku = &o.tools[0][0];
    let f = floor("opus high");
    assert_eq!(choose_model(0, Some(&f), haiku, "high"), None);
}

#[test]
fn a_cell_with_no_model_offers_the_tool_default_for_other_tools_only() {
    let o = options();
    let with = o.models(1, true);
    assert_eq!(with[0].id, NO_MODEL);
    assert!(with[0].efforts.contains(&"low".to_owned()));
    assert!(o.models(1, false).iter().all(|m| m.id != NO_MODEL));
    assert!(o.models(0, true).iter().all(|m| m.id != NO_MODEL));
}

#[test]
fn a_pick_is_model_then_effort() {
    assert_eq!(pick_text("opus", "high"), "opus high");
    assert_eq!(pick_text(NO_MODEL, "low"), "- low");
}
