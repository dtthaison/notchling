use super::*;
use serde_json::json;

fn failover() -> &'static KeySpec {
    spec("clusterFailover").expect("the clusterFailover key")
}

#[test]
fn failover_key_defaults_off_with_three_choices() {
    let key = failover();
    assert_eq!(key.default, Fallback::Str("off"));
    assert_eq!(key.default_value(), Some(json!("off")));
    assert_eq!(key.allowed, ["off", "30m", "1h"]);
    assert!(key.accepts(&json!("30m")));
    assert!(key.accepts(&json!("1h")));
    assert!(!key.accepts(&json!("2h")));
    assert!(!key.accepts(&json!(true)));
    assert!(!key.from_awake);
    assert_eq!(key.from_text("1H"), json!("1h"));
}

#[test]
fn failover_key_is_class_ask() {
    let key = failover();
    assert_eq!(key.access, Access::Ask);
    assert!(key.access.shown());
    assert_eq!(key.title, "Automatic Failover");
    assert_eq!(
        key.about,
        "When the primary Mac can't be reached for this long, a Standby takes over its sessions"
    );
    assert_eq!(key.why, "it changes what runs where");
    assert_eq!(key.access.as_str(), "ask");
}

#[test]
fn attention_level_has_three_choices_and_is_free_for_agents() {
    let key = spec("attentionLevel").expect("the attentionLevel key");
    assert_eq!(key.default, Fallback::Str("normal"));
    assert_eq!(key.allowed, ["calm", "normal", "urgent"]);
    assert!(key.accepts(&json!("urgent")));
    assert!(!key.accepts(&json!("loud")));
    assert_eq!(key.access, Access::Free);
    assert!(!key.from_awake);
}
