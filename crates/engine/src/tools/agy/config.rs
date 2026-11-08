//! What `agy acp` tells a session besides its conversation: the choices it offers (Model, Thinking
//! Level, Mode), its slash commands, and a permission it is waiting on (Swift `ACPConfigOption`,
//! `ACPPermission`, `SlashCommand` as ACPRunner fills them).

use crate::bridge::foreign::PermissionOption;
use crate::transcript::acp::tool_call;
use crate::transcript::model::{DiffKind, ToolCall};
use serde_json::{json, Value};

/// One value a choice can take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigValue {
    pub value: String,
    pub name: String,
}

/// A choice the agent offers for the session: `configOptions` of `session/load` and `session/new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigOption {
    pub id: String,
    pub name: String,
    pub current: String,
    pub values: Vec<ConfigValue>,
}

impl ConfigOption {
    /// The `select` options of a `configOptions` list; others are skipped.
    pub fn parse(list: Option<&Value>) -> Vec<ConfigOption> {
        let Some(list) = list.and_then(Value::as_array) else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|o| {
                let id = o.get("id")?.as_str()?;
                if o.get("type").and_then(Value::as_str) != Some("select") {
                    return None;
                }
                let values = o
                    .get("options")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(|v| {
                        let value = v.get("value")?.as_str()?;
                        Some(ConfigValue {
                            value: value.to_string(),
                            name: v
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or(value)
                                .into(),
                        })
                    })
                    .collect();
                Some(ConfigOption {
                    id: id.to_string(),
                    name: o.get("name").and_then(Value::as_str).unwrap_or(id).into(),
                    current: o
                        .get("currentValue")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                    values,
                })
            })
            .collect()
    }

    /// The current value's name, else the value itself.
    pub fn current_name(&self) -> &str {
        self.values
            .iter()
            .find(|v| v.value == self.current)
            .map_or(self.current.as_str(), |v| v.name.as_str())
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "name": self.name, "current": self.current,
            "values": self.values.iter().map(|v| json!({"value": v.value, "name": v.name})).collect::<Vec<_>>(),
        })
    }
}

/// A slash command the agent offers (`available_commands_update`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgyCommand {
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
}

impl AgyCommand {
    pub fn parse(list: Option<&Value>) -> Vec<AgyCommand> {
        let Some(list) = list.and_then(Value::as_array) else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|c| {
                Some(AgyCommand {
                    name: c.get("name")?.as_str()?.to_string(),
                    description: c
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                    argument_hint: c
                        .get("input")
                        .and_then(|i| i.get("hint"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
            })
            .collect()
    }

    pub fn to_json(&self) -> Value {
        json!({"name": self.name, "description": self.description, "argumentHint": self.argument_hint})
    }
}

/// A permission the agent asked for mid-turn (`session/request_permission`), waiting for the user.
#[derive(Debug, Clone, PartialEq)]
pub struct Permission {
    /// The JSON-RPC id to answer, a string or a number, kept as it came.
    pub rpc_id: Value,
    pub call: ToolCall,
    pub options: Vec<PermissionOption>,
}

impl Permission {
    /// From the request's `toolCall` and `options`; `None` without a tool call object.
    pub fn parse(rpc_id: &Value, params: &Value) -> Option<Permission> {
        let call = params.get("toolCall").filter(|c| c.is_object())?;
        let options = params
            .get("options")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .filter_map(|o| {
                let id = o.get("optionId")?.as_str()?;
                Some(PermissionOption {
                    id: id.to_string(),
                    name: o.get("name").and_then(Value::as_str).unwrap_or(id).into(),
                    kind: o.get("kind").and_then(Value::as_str).unwrap_or("").into(),
                })
            })
            .collect();
        Some(Permission {
            rpc_id: rpc_id.clone(),
            call: tool_call(call, None),
            options,
        })
    }

    /// The id as text, the way a caller names it (`1` for `"1"` and for `1` alike).
    pub fn rpc_text(&self) -> String {
        rpc_text(&self.rpc_id)
    }

    pub fn option(&self, id: &str) -> Option<&PermissionOption> {
        self.options.iter().find(|o| o.id == id)
    }

    /// What the tool would do, for a card: id, name, title, input, diff and the options.
    pub fn to_json(&self) -> Value {
        let diff: Vec<Value> = self
            .call
            .diff
            .iter()
            .map(|d| {
                json!({"kind": match d.kind {
                    DiffKind::Add => "add",
                    DiffKind::Remove => "remove",
                    DiffKind::Context => "context",
                    DiffKind::Hunk => "hunk",
                }, "text": d.text})
            })
            .collect();
        json!({
            "rpcId": self.rpc_text(),
            "toolName": self.call.name,
            "title": self.call.title,
            "input": self.call.input,
            "diff": diff,
            "options": self.options.iter().map(|o| json!({"id": o.id, "name": o.name, "kind": o.kind})).collect::<Vec<_>>(),
        })
    }
}

/// A JSON-RPC id as text: a string as it is, anything else as JSON.
pub fn rpc_text(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_reads_select_options_and_skips_the_rest() {
        let list = json!([
            {"id": "model", "name": "Model", "type": "select", "currentValue": "b",
             "options": [{"value": "a", "name": "Alpha"}, {"value": "b"}, {"name": "no value"}]},
            {"id": "flag", "type": "toggle"},
            {"type": "select"},
            {"id": "mode", "type": "select"},
        ]);
        let options = ConfigOption::parse(Some(&list));
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].values.len(), 2);
        assert_eq!(options[0].current_name(), "b", "no name: the value");
        assert_eq!(options[1].name, "mode");
        assert_eq!(options[1].current_name(), "");
        assert!(ConfigOption::parse(None).is_empty());
        assert!(ConfigOption::parse(Some(&json!("x"))).is_empty());
    }

    #[test]
    fn a_permission_keeps_its_id_type_and_lists_the_agents_own_options() {
        let params = json!({
        "toolCall": {"toolCallId": "c", "name": "edit_file", "kind": "edit", "title": "File a.txt",
            "content": [{"type": "diff", "path": "a.txt", "newText": "new", "oldText": "old"}]},
        "options": [
            {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
            {"optionId": "deny", "name": "Deny", "kind": "reject_once"},
            {"name": "no id"},
        ]});
        let p = Permission::parse(&json!("1"), &params).unwrap();
        assert_eq!(p.rpc_id, json!("1"));
        assert_eq!(p.rpc_text(), "1");
        assert_eq!(p.options.len(), 2);
        assert!(p.option("allow").unwrap().allows());
        assert!(!p.option("deny").unwrap().allows());
        assert!(p.option("other").is_none());
        let view = p.to_json();
        assert_eq!(view["rpcId"], "1");
        assert_eq!(view["diff"][0]["kind"], "remove");
        assert_eq!(view["diff"][1]["text"], "new");
        assert_eq!(
            Permission::parse(&json!(4), &params).unwrap().rpc_text(),
            "4"
        );
        assert!(Permission::parse(&json!(1), &json!({"options": []})).is_none());
    }

    #[test]
    fn commands_read_their_hint() {
        let list = json!([{"name": "goal", "description": "d", "input": {"hint": "<goal>"}}, {"description": "x"}]);
        let c = AgyCommand::parse(Some(&list));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].argument_hint.as_deref(), Some("<goal>"));
    }
}
