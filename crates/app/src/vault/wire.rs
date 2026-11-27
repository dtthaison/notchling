use notchling_engine::vault::entry::{PrefillChanges, VaultEntry, VaultRequest, VaultValue};
use notchling_local_api::ops;
use serde_json::{json, Value};

#[derive(Clone, PartialEq)]
pub struct Call {
    pub op: &'static str,
    pub args: Value,
}

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

    pub fn save(
        entry: &VaultEntry,
        value: Option<&VaultValue>,
        replacing: Option<&str>,
        consent: Option<&str>,
    ) -> Call {
        let mut args = json!({ "entry": entry });
        if let Some(v) = value {
            args["value"] = json!({ "username": v.username, "secret": v.secret });
        }
        if let Some(old) = replacing {
            args["replacing"] = json!(old);
        }
        if let Some(id) = consent {
            args["consentId"] = json!(id);
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

    pub fn consent_get(id: &str) -> Call {
        Call::new(ops::CONSENT_GET, json!({ "id": id }))
    }
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    pub exe: Option<String>,
    pub pid: Option<i64>,
    pub cwd: Option<String>,
    pub parents: Vec<String>,
    pub folder_matches: Option<bool>,
}

impl Verified {
    pub fn from_value(v: &Value) -> Option<Verified> {
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
            folder_matches: v.get("folder_matches").and_then(Value::as_bool),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approval {
    pub id: String,
    pub names: Vec<String>,
    pub command: String,
    pub folder: String,
    pub verified: Option<Verified>,
    pub machine: Option<String>,
    pub session: Option<String>,
}

fn session_of(data: &Value) -> Option<String> {
    data.get("session")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
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
            machine: data
                .get("machine")
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty())
                .map(str::to_owned),
            session: session_of(data),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingRequest {
    pub id: String,
    pub request: VaultRequest,
    pub entry: VaultEntry,
    pub existing: bool,
    pub changes: PrefillChanges,
    pub session: Option<String>,
    pub verified: Option<Verified>,
    pub snapshot: Option<Snapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub existing: bool,
    pub revision: Option<String>,
}

impl Snapshot {
    fn from_event(data: &Value) -> Option<Snapshot> {
        Some(Snapshot {
            existing: data.get("existing")?.as_bool()?,
            revision: data
                .get("revision")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }
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
            session: session_of(data),
            verified: data.get("verified").and_then(Verified::from_value),
            snapshot: Snapshot::from_event(data),
        })
    }
}

impl PendingRequest {
    pub fn twin_of(&self, other: &PendingRequest) -> bool {
        self.id != other.id
            && self.session.is_some()
            && self.session == other.session
            && self.snapshot.is_some()
            && self.snapshot == other.snapshot
            && self.request.name.to_lowercase() == other.request.name.to_lowercase()
    }

    pub fn twin_ids(&self, events: &[Value]) -> Vec<String> {
        events
            .iter()
            .filter_map(PendingRequest::from_event)
            .filter(|other| self.twin_of(other))
            .map(|other| other.id)
            .collect()
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
        assert_eq!(save.args["consentId"], "r1");
        assert!(save.args.get("request").is_none());
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
        assert_eq!(Call::consent_get("c1").op, "consent.get");
        assert_eq!(Call::consent_get("c1").args, json!({"id": "c1"}));
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
        assert_eq!(a.session, None);
        let named = Approval::from_event(&json!({"id": "ap-2", "session": "s-1"})).unwrap();
        assert_eq!(named.session.as_deref(), Some("s-1"));
        let blank = Approval::from_event(&json!({"id": "ap-3", "session": ""})).unwrap();
        assert_eq!(blank.session, None);
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
    fn asked(id: &str, name: &str, session: Option<&str>, existing: bool) -> PendingRequest {
        asked_at(id, name, session, existing, None)
    }

    fn asked_at(
        id: &str,
        name: &str,
        session: Option<&str>,
        existing: bool,
        revision: Option<&str>,
    ) -> PendingRequest {
        PendingRequest::from_event(&json!({"id": id, "session": session,
            "request": {"name": name}, "entry": entry(name), "existing": existing,
            "revision": revision}))
        .unwrap()
    }

    #[test]
    fn a_re_ask_of_the_same_secret_in_the_same_session_is_a_twin() {
        let first = asked("c1", "GitHub", Some("s-1"), true);
        assert!(first.twin_of(&asked("c2", "github", Some("s-1"), true)));
        assert!(!first.twin_of(&first), "never its own twin");
        assert!(!first.twin_of(&asked("c2", "github", Some("s-2"), true)));
        assert!(!first.twin_of(&asked("c2", "npm", Some("s-1"), true)));
        assert!(!first.twin_of(&asked("c2", "github", Some("s-1"), false)));
        let anonymous = asked("c3", "github", None, true);
        assert!(!anonymous.twin_of(&asked("c4", "github", None, true)));
    }

    #[test]
    fn only_a_request_recorded_against_the_same_snapshot_is_a_twin() {
        let first = asked_at("c1", "github", Some("s-1"), true, Some("r1"));
        assert!(first.twin_of(&asked_at("c2", "github", Some("s-1"), true, Some("r1"))));
        assert!(!first.twin_of(&asked_at("c2", "github", Some("s-1"), true, Some("r2"))));
        assert!(!first.twin_of(&asked_at("c2", "github", Some("s-1"), true, None)));
        let fresh = asked_at("c3", "github", Some("s-1"), false, None);
        assert!(fresh.twin_of(&asked_at("c4", "github", Some("s-1"), false, None)));
        assert!(!fresh.twin_of(&first));
        let unknown = PendingRequest::from_event(&json!({"id": "c5", "session": "s-1",
            "request": {"name": "github"}, "entry": entry("github"), "existing": null}))
        .unwrap();
        let other = PendingRequest::from_event(&json!({"id": "c6", "session": "s-1",
            "request": {"name": "github"}, "entry": entry("github"), "existing": null}))
        .unwrap();
        assert!(
            !unknown.twin_of(&other),
            "an unresolved snapshot joins nothing"
        );
    }

    #[test]
    fn a_cancel_names_every_live_twin_of_the_form_and_nothing_else() {
        let form = asked_at("c1", "github", Some("s-1"), true, Some("r1"));
        let event = |id: &str, name: &str, session: &str, revision: &str| {
            json!({"id": id, "session": session, "request": {"name": name},
                   "entry": entry(name), "existing": true, "revision": revision})
        };
        let pool = [
            event("c1", "github", "s-1", "r1"),
            event("c2", "GitHub", "s-1", "r1"),
            event("c3", "github", "s-1", "r2"),
            event("c4", "github", "s-2", "r1"),
            event("c5", "npm", "s-1", "r1"),
            event("c6", "github", "s-1", "r1"),
        ];
        assert_eq!(form.twin_ids(&pool), ["c2", "c6"]);
        assert!(form.twin_ids(&[]).is_empty());
    }
}
