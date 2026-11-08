//! The Secret Vault's app-tier ops as the app sends and reads them (`notchlingd`'s `vault_ops`
//! module docs). No reply read here carries a value: `vault.use` is the agents' op and the app
//! never calls it. The pending events (`vault.approval.pending`, `vault.request.pending`) are
//! read tolerantly; one without an id is nothing to answer.

use notchling_engine::vault::entry::{PrefillChanges, VaultEntry, VaultRequest, VaultValue};
use notchling_local_api::ops;
use serde_json::{json, Value};

/// One op and its arguments.
#[derive(Clone, PartialEq)]
pub struct Call {
    pub op: &'static str,
    pub args: Value,
}

/// A call's `Debug` never shows a value: `vault.save`'s `value` is printed as hidden.
impl std::fmt::Debug for Call {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut args = self.args.clone();
        if let Some(value) = args.get_mut("value") {
            *value = json!("<hidden>");
        }
        f.debug_struct("Call")
            .field("op", &self.op)
            .field("args", &args)
            .finish()
    }
}

/// An approval's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Deny,
    Once,
    Always,
}

impl Decision {
    pub fn wire(self) -> &'static str {
        match self {
            Decision::Deny => "deny",
            Decision::Once => "once",
            Decision::Always => "always",
        }
    }
}

impl Call {
    fn new(op: &'static str, args: Value) -> Call {
        Call { op, args }
    }

    pub fn list() -> Call {
        Call::new(ops::VAULT_LIST, json!({}))
    }

    /// Saves `entry` with `value` (left out to keep the stored value), `replacing` the entry it
    /// renames, answering `request` (a `vault.request.pending` id) when an agent asked.
    pub fn save(
        entry: &VaultEntry,
        value: Option<&VaultValue>,
        replacing: Option<&str>,
        request: Option<&str>,
    ) -> Call {
        let mut args = json!({ "entry": entry });
        if let Some(v) = value {
            args["value"] = json!({ "username": v.username, "secret": v.secret });
        }
        if let Some(old) = replacing {
            args["replacing"] = json!(old);
        }
        if let Some(id) = request {
            args["request"] = json!(id);
        }
        Call::new(ops::VAULT_SAVE, args)
    }

    pub fn remove(name: &str) -> Call {
        Call::new(ops::VAULT_REMOVE, json!({ "name": name }))
    }

    pub fn approval_answer(id: &str, decision: Decision) -> Call {
        Call::new(
            ops::VAULT_APPROVAL_ANSWER,
            json!({ "id": id, "decision": decision.wire() }),
        )
    }

    pub fn request_cancel(id: &str) -> Call {
        Call::new(ops::VAULT_REQUEST_CANCEL, json!({ "id": id }))
    }
}

/// `vault.list`'s entries, sorted by name as the list shows them. An entry that doesn't read is
/// skipped, never the whole list.
pub fn parse_entries(v: &Value) -> Vec<VaultEntry> {
    let mut entries: Vec<VaultEntry> = v
        .get("entries")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| serde_json::from_value(e.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by_key(|e| e.name.to_lowercase());
    entries
}

/// What the daemon checked about the process asking (`verified` on `vault.approval.pending`):
/// read from the socket's peer, not from anything the agent said. Older daemons don't send it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    /// The caller's executable.
    pub exe: Option<String>,
    pub pid: Option<i64>,
    /// Its real working folder.
    pub cwd: Option<String>,
    /// Process names, nearest first.
    pub parents: Vec<String>,
    /// Whether the folder the request names is where it runs.
    pub folder_matches: bool,
}

impl Verified {
    fn from_value(v: &Value) -> Option<Verified> {
        v.as_object()?;
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        Some(Verified {
            exe: text("exe"),
            pid: v.get("pid").and_then(Value::as_i64),
            cwd: text("cwd"),
            parents: v
                .get("parents")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|n| n.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            // Missing is not a match: nothing was checked.
            folder_matches: v
                .get("folder_matches")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }
}

/// An agent waiting to use secrets marked ask-first (`vault.approval.pending`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approval {
    pub id: String,
    pub names: Vec<String>,
    pub command: String,
    pub folder: String,
    pub verified: Option<Verified>,
}

impl Approval {
    pub fn from_event(data: &Value) -> Option<Approval> {
        let id = data.get("id")?.as_str()?.to_owned();
        if id.is_empty() {
            return None;
        }
        let names = data
            .get("names")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|n| n.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let text = |key: &str| {
            data.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Some(Approval {
            id,
            names,
            command: text("command"),
            folder: text("folder"),
            verified: data.get("verified").and_then(Verified::from_value),
        })
    }
}

/// An agent asking for a secret to be added or updated (`vault.request.pending`): the form, filled
/// in by the daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingRequest {
    pub id: String,
    pub request: VaultRequest,
    /// The form's entry: the saved one with the request laid over it, or a new one.
    pub entry: VaultEntry,
    /// Whether a saved entry of that name exists.
    pub existing: bool,
    pub changes: PrefillChanges,
}

impl PendingRequest {
    pub fn from_event(data: &Value) -> Option<PendingRequest> {
        let id = data.get("id")?.as_str()?.to_owned();
        if id.is_empty() {
            return None;
        }
        let entry: VaultEntry = serde_json::from_value(data.get("entry")?.clone()).ok()?;
        let request = data
            .get("request")
            .and_then(|r| serde_json::from_value(r.clone()).ok())
            .unwrap_or_default();
        let changes = data
            .get("changes")
            .and_then(|c| serde_json::from_value(c.clone()).ok())
            .unwrap_or_default();
        Some(PendingRequest {
            id,
            request,
            entry,
            existing: data
                .get("existing")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            changes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::vault::entry::Kind;

    fn entry(name: &str) -> VaultEntry {
        VaultEntry::new(name, Kind::Token)
    }

    #[test]
    fn the_calls_have_the_module_docs_shapes_and_never_log_a_value() {
        let value = VaultValue {
            username: String::new(),
            secret: "s3cret".into(),
        };
        let save = Call::save(&entry("github"), Some(&value), Some("gh"), Some("r1"));
        assert_eq!(save.op, "vault.save");
        assert_eq!(save.args["entry"]["name"], "github");
        assert_eq!(save.args["entry"]["secretVar"], "GITHUB_TOKEN");
        assert_eq!(save.args["value"]["secret"], "s3cret");
        assert_eq!(save.args["replacing"], "gh");
        assert_eq!(save.args["request"], "r1");
        let keep = Call::save(&entry("github"), None, None, None);
        assert!(
            keep.args.get("value").is_none(),
            "no value keeps the stored one"
        );
        assert!(keep.args.get("replacing").is_none());
        assert_eq!(Call::remove("github").args, json!({"name": "github"}));
        assert_eq!(
            Call::approval_answer("ap-1", Decision::Always).args,
            json!({"id": "ap-1", "decision": "always"})
        );
        assert_eq!(Call::request_cancel("r1").args, json!({"id": "r1"}));
        assert_eq!(Call::list().op, "vault.list");
        assert!(!format!("{value:?}").contains("s3cret"), "Debug hides it");
        let shown = format!("{save:?}");
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(
            shown.contains("vault.save") && shown.contains("<hidden>"),
            "{shown}"
        );
    }

    #[test]
    fn entries_are_read_and_sorted_and_the_pending_events_need_an_id() {
        let list = parse_entries(&json!({"entries": [
            json!(entry("npm")), json!(entry("GitHub")), {"name": "broken"}
        ]}));
        let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["GitHub", "npm"]);
        let a = Approval::from_event(&json!({"id": "ap-1", "names": ["github", "npm"],
            "command": "gh api", "folder": "/w"}))
        .unwrap();
        assert_eq!(a.names, ["github", "npm"]);
        assert_eq!((a.command.as_str(), a.folder.as_str()), ("gh api", "/w"));
        assert_eq!(Approval::from_event(&json!({"names": ["x"]})), None);
        let r = PendingRequest::from_event(&json!({"id": "r1",
            "request": {"name": "github", "reason": "to push"},
            "entry": entry("github"), "existing": true,
            "changes": {"old": {"note": "was this"}, "hostChange": null}}))
        .unwrap();
        assert!(r.existing);
        assert_eq!(r.request.reason.as_deref(), Some("to push"));
        assert_eq!(r.changes.old.len(), 1);
        assert_eq!(
            PendingRequest::from_event(&json!({"id": "r2"})),
            None,
            "no entry"
        );
    }
}
