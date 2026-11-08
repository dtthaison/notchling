//! An Antigravity session's options picker, pure (`ACPOptionsPicker`): the choices the session
//! offers (Model, Thinking Level, Mode) as `agy.state` lists them, the composer's label, and the
//! `agy.set_option` a pick sends. A session not started yet has none; a pick applies from the
//! next turn.

use notchling_engine::tools::agy::{ConfigOption, ConfigValue};
use serde_json::Value;

pub const NOTE: &str = "Applies from the next turn.";
pub const DRAFT_TIP: &str =
    "Your first message starts the session; then its model and mode can be picked here.";
pub const TIP: &str = "The model, thinking level and mode for this session's next turns.";

/// The `config` list of an `agy.state` reply (`ConfigOption::to_json`'s shape).
pub fn parse_config(state: &Value) -> Vec<ConfigOption> {
    let Some(list) = state.get("config").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|o| {
            let text = |key: &str| o.get(key).and_then(Value::as_str).map(str::to_owned);
            Some(ConfigOption {
                id: text("id")?,
                name: text("name").unwrap_or_default(),
                current: text("current").unwrap_or_default(),
                values: o
                    .get("values")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|v| {
                                let value = v.get("value")?.as_str()?.to_owned();
                                Some(ConfigValue {
                                    name: v
                                        .get("name")
                                        .and_then(Value::as_str)
                                        .unwrap_or(&value)
                                        .to_owned(),
                                    value,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// The composer's label: "Antigravity · Gemini 3 Pro · High", or the tool's name alone while the
/// session isn't started or still loading ("Antigravity · Loading…").
pub fn label(options: &[ConfigOption], started: bool, loading: bool) -> String {
    if !started {
        return "Antigravity".to_owned();
    }
    let parts: Vec<&str> = options
        .iter()
        .filter(|o| o.id == "model" || o.id == "thought_level")
        .map(ConfigOption::current_name)
        .filter(|name| !name.is_empty())
        .collect();
    if parts.is_empty() {
        return if loading {
            "Antigravity · Loading…".to_owned()
        } else {
            "Antigravity".to_owned()
        };
    }
    let mut all = vec!["Antigravity"];
    all.extend(parts);
    all.join(" · ")
}

/// What the picker's hover says.
pub fn tip(started: bool) -> &'static str {
    if started {
        TIP
    } else {
        DRAFT_TIP
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state() -> Value {
        json!({"config": [
            {"id": "model", "name": "Model", "current": "g3", "values": [
                {"value": "g3", "name": "Gemini 3 Pro"}, {"value": "g3f"}]},
            {"id": "thought_level", "name": "Thinking Level", "current": "high", "values": [
                {"value": "low", "name": "Low"}, {"value": "high", "name": "High"}]},
            {"id": "mode", "name": "Mode", "current": "agent", "values": [{"value": "agent", "name": "Agent"}]},
            {"name": "no id"}
        ]})
    }

    #[test]
    fn options_are_read_from_the_state_and_labelled() {
        let options = parse_config(&state());
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].current_name(), "Gemini 3 Pro");
        assert_eq!(options[0].values[1].name, "g3f", "no name: the value");
        assert_eq!(
            label(&options, true, false),
            "Antigravity · Gemini 3 Pro · High"
        );
        assert_eq!(label(&options, false, false), "Antigravity");
        assert_eq!(label(&[], true, true), "Antigravity · Loading…");
        assert_eq!(label(&[], true, false), "Antigravity");
        assert!(parse_config(&json!({})).is_empty());
        assert_eq!(tip(false), DRAFT_TIP);
        assert_eq!(tip(true), TIP);
    }
}
