//! The daemon's own list of saved accounts (the Swift app kept these in UserDefaults, which the
//! daemon doesn't touch). Names only: an id, an email and a plan. The logins themselves are in the
//! Keychain. A file here is the daemon's, in its own folder, 0600 and never written through a link.

use serde_json::{json, Value};
use std::path::Path;

/// A saved account as the daemon lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub id: String,
    pub email: String,
    pub plan: Option<String>,
}

/// The accounts in `file`; empty when it is missing or isn't ours.
pub fn read_accounts(file: &Path) -> Vec<Saved> {
    let Some(root) = read_json(file) else {
        return vec![];
    };
    root.get("accounts")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    Some(Saved {
                        id: a.get("id")?.as_str()?.to_string(),
                        email: a.get("email")?.as_str()?.to_string(),
                        plan: a.get("plan").and_then(Value::as_str).map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Writes the list; false when it couldn't be written.
pub fn write_accounts(file: &Path, accounts: &[Saved]) -> bool {
    let list: Vec<Value> = accounts
        .iter()
        .map(|a| json!({"id": a.id, "email": a.email, "plan": a.plan}))
        .collect();
    write_json(file, &json!({"version": 1, "accounts": list}))
}

/// The ids in `file` (Claude Desktop's accounts it has cookies for).
pub fn read_ids(file: &Path) -> Vec<String> {
    read_json(file)
        .and_then(|root| {
            root.get("ids").and_then(Value::as_array).map(|list| {
                list.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default()
}

pub fn write_ids(file: &Path, ids: &[String]) -> bool {
    write_json(file, &json!({"version": 1, "ids": ids}))
}

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

fn write_json(file: &Path, value: &Value) -> bool {
    let Some(dir) = file.parent() else {
        return false;
    };
    if notchling_platform::files::ensure_private_dir(dir).is_err() {
        return false;
    }
    let Ok(data) = serde_json::to_vec_pretty(value) else {
        return false;
    };
    notchling_platform::files::write_private_no_follow(file, &data).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_round_trip_and_a_foreign_file_reads_as_empty() {
        let dir = std::env::temp_dir().join(format!("nl-book-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("accounts").join("codex.json");
        assert!(read_accounts(&file).is_empty());
        let list = vec![
            Saved {
                id: "u:w".into(),
                email: "a@example.com".into(),
                plan: Some("Plus".into()),
            },
            Saved {
                id: "x:y".into(),
                email: "b@example.com".into(),
                plan: None,
            },
        ];
        assert!(write_accounts(&file, &list));
        assert_eq!(read_accounts(&file), list);
        std::fs::write(&file, "[1,2]").unwrap();
        assert!(read_accounts(&file).is_empty());
        let ids = dir.join("ids.json");
        assert!(write_ids(&ids, &["a".into(), "b".into()]));
        assert_eq!(read_ids(&ids), vec!["a", "b"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
