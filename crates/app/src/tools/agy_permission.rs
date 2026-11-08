//! An Antigravity permission as the daemon publishes it (`prompt.pending` with `tool: "agy"`,
//! plus `rpcId`, `options` and `diff`): what the tool would do, and Antigravity's own options in
//! the order the notch lists them (`NotchForeign.orderedOptions`, `preview`, `AgyPermissionCard`).
//! Answering goes through `agy.permission` by the row id, the rpc id and the option picked.

use notchling_engine::bridge::foreign::PermissionOption;
use notchling_engine::transcript::model::{DiffKind, DiffLine};
use serde_json::Value;

/// The card shows at most this many lines of the diff or the input.
pub const PREVIEW_LINES: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct AgyPermission {
    /// The JSON-RPC id to answer, as text.
    pub rpc_id: String,
    /// What the tool would do ("File hello.txt"), else its name.
    pub title: String,
    pub tool_name: String,
    /// The command or input, as plain text.
    pub input: String,
    pub diff: Vec<DiffLine>,
    pub options: Vec<PermissionOption>,
}

fn text(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

impl AgyPermission {
    /// From a held prompt's data; `None` unless it is Antigravity's with an rpc id and options.
    pub fn from_prompt(p: &Value) -> Option<AgyPermission> {
        if p.get("tool").and_then(Value::as_str) != Some("agy") {
            return None;
        }
        let rpc_id = p.get("rpcId")?.as_str()?.to_owned();
        let options: Vec<PermissionOption> = p
            .get("options")?
            .as_array()?
            .iter()
            .filter_map(|o| {
                let id = o.get("id")?.as_str()?;
                Some(PermissionOption {
                    id: id.to_owned(),
                    name: o
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(id)
                        .to_owned(),
                    kind: text(o, "kind"),
                })
            })
            .collect();
        if options.is_empty() {
            return None;
        }
        let diff = p
            .get("diff")
            .and_then(Value::as_array)
            .map(|lines| {
                lines
                    .iter()
                    .map(|d| DiffLine {
                        kind: match d.get("kind").and_then(Value::as_str) {
                            Some("add") => DiffKind::Add,
                            Some("remove") => DiffKind::Remove,
                            Some("hunk") => DiffKind::Hunk,
                            _ => DiffKind::Context,
                        },
                        text: text(d, "text"),
                        number: None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let tool_name = text(p, "toolName");
        let summary = text(p, "summary");
        Some(AgyPermission {
            rpc_id,
            title: if summary.is_empty() {
                tool_name.clone()
            } else {
                summary
            },
            tool_name,
            input: text(p, "input"),
            diff,
            options,
        })
    }

    /// Antigravity's options in the order the card lists them: the one-time allow first, other
    /// allows next, then the rejections, so the first row is the safe default a Return would take.
    pub fn ordered_options(&self) -> Vec<&PermissionOption> {
        let rank = |o: &PermissionOption| match o.kind.as_str() {
            "allow_once" => 0,
            "allow_always" => 1,
            "reject_once" => 3,
            "reject_always" => 4,
            _ if o.allows() => 2,
            _ => 5,
        };
        let mut indexed: Vec<(usize, &PermissionOption)> =
            self.options.iter().enumerate().collect();
        indexed.sort_by_key(|(i, o)| (rank(o), *i));
        indexed.into_iter().map(|(_, o)| o).collect()
    }

    /// What the card shows of the call: the edit's diff, else the command or input, a few lines.
    pub fn preview(&self, max_lines: usize) -> Vec<(String, DiffKind)> {
        if !self.diff.is_empty() {
            return self
                .diff
                .iter()
                .take(max_lines)
                .map(|d| (d.text.clone(), d.kind))
                .collect();
        }
        let source = if self.input.is_empty() {
            &self.tool_name
        } else {
            &self.input
        };
        source
            .split('\n')
            .take(max_lines)
            .map(|l| (l.to_owned(), DiffKind::Context))
            .collect()
    }

    /// The card's estimated height (`NotchForeign.permissionHeight`): header, title, the preview
    /// box, the options and the "Answer in Chat" line.
    pub fn height(&self) -> f64 {
        let lines = self.preview(PREVIEW_LINES).len() as f64;
        28.0 + 20.0 + 18.0 + lines * 15.0 + 22.0 + self.options.len() as f64 * 36.0 + 28.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn prompt() -> Value {
        json!({
            "id": "agy-perm:agy:s1:1", "sessionId": "agy:s1", "tool": "agy", "kind": "permission",
            "toolName": "edit_file", "summary": "File hello.txt", "input": "hello.txt",
            "canAlwaysAllow": true, "rpcId": "1",
            "options": [
                {"id": "reject", "name": "Reject", "kind": "reject_once"},
                {"id": "always", "name": "Always allow", "kind": "allow_always"},
                {"id": "allow", "name": "Allow", "kind": "allow_once"},
                {"id": "never", "name": "Never", "kind": "reject_always"}
            ],
            "diff": [{"kind": "remove", "text": "hello"}, {"kind": "add", "text": "hello there"}]
        })
    }

    #[test]
    fn it_reads_the_daemons_prompt_and_orders_the_options() {
        let p = AgyPermission::from_prompt(&prompt()).unwrap();
        assert_eq!(
            (p.rpc_id.as_str(), p.title.as_str()),
            ("1", "File hello.txt")
        );
        assert_eq!(p.diff.len(), 2);
        assert_eq!(p.diff[1].kind, DiffKind::Add);
        let order: Vec<&str> = p.ordered_options().iter().map(|o| o.id.as_str()).collect();
        assert_eq!(order, ["allow", "always", "reject", "never"]);
        assert_eq!(
            p.preview(8),
            vec![
                ("hello".to_owned(), DiffKind::Remove),
                ("hello there".to_owned(), DiffKind::Add)
            ]
        );
        assert_eq!(
            p.height(),
            28.0 + 20.0 + 18.0 + 30.0 + 22.0 + 4.0 * 36.0 + 28.0
        );
    }

    #[test]
    fn a_command_previews_its_input_and_other_prompts_are_not_antigravitys() {
        let mut p = prompt();
        p["diff"] = json!([]);
        p["input"] = json!("rm -rf build\nls");
        p["summary"] = json!("");
        let p = AgyPermission::from_prompt(&p).unwrap();
        assert_eq!(p.title, "edit_file", "no summary: the tool's name");
        assert_eq!(
            p.preview(1),
            vec![("rm -rf build".to_owned(), DiffKind::Context)]
        );
        let mut claude = prompt();
        claude["tool"] = json!("claude");
        assert_eq!(AgyPermission::from_prompt(&claude), None);
        let mut no_options = prompt();
        no_options["options"] = json!([]);
        assert_eq!(AgyPermission::from_prompt(&no_options), None);
    }
}
