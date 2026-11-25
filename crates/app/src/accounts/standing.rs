use std::collections::BTreeMap;

use notchling_engine::providers::usage::Standing;
use serde_json::Value;

pub fn parse(v: &Value) -> BTreeMap<String, Standing> {
    v.get("standing")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(id, s)| {
                    serde_json::from_value::<Standing>(s.clone())
                        .ok()
                        .filter(|s| !s.is_empty())
                        .map(|s| (id.clone(), s))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::providers::usage::{Credits, Reached};
    use serde_json::json;

    #[test]
    fn a_reply_with_a_standing_per_account_is_read_and_nothing_else_is_kept() {
        let reply = json!({"provider": "codex", "reports": {}, "standing": {
            "u:a": {"reached": "credits", "credits": null},
            "u:b": {"reached": null, "credits": {"unlimited": false, "balance": "7"}},
            "u:c": {"reached": null, "credits": null},
            "u:d": {"reached": "something new"},
            "u:e": 5}});
        let read = parse(&reply);
        assert_eq!(read.len(), 2, "{read:?}");
        assert_eq!(read["u:a"].reached, Some(Reached::Credits));
        assert_eq!(
            read["u:b"].credits,
            Some(Credits {
                unlimited: false,
                balance: Some("7".into())
            })
        );
    }

    #[test]
    fn an_older_daemon_without_a_standing_gives_an_empty_map() {
        assert!(parse(&json!({"provider": "codex", "reports": {}})).is_empty());
        assert!(parse(&json!({"standing": []})).is_empty());
        assert!(parse(&Value::Null).is_empty());
    }
}
