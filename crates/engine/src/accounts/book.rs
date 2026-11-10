//! The daemon's own list of saved accounts (the Swift app kept these in UserDefaults, which the
//! daemon doesn't touch). Names only: an id, an email and a plan. The logins themselves are in the
//! Keychain. Each list is a document in `<data>/notchling.db` (`crate::docfile`, decision 0014):
//! `accounts/codex` and `accounts/antigravity` (the files `<data>/daemon/accounts/codex.json` and
//! `antigravity.json` they used to be), and `accounts/ids` (Claude Desktop's accounts it has
//! cookies for, once `ids.json`). The old file moves in once, on the first change; a read takes
//! the document, else the old file, and writes nothing. A change is one transaction.

use crate::docfile::DocFile;
use serde_json::{json, Value};

/// The document of Codex's saved accounts.
pub const CODEX_DOC: &str = "accounts/codex";
/// The document of Antigravity's saved accounts.
pub const ANTIGRAVITY_DOC: &str = "accounts/antigravity";
/// The document of the Claude Desktop account ids.
pub const IDS_DOC: &str = "accounts/ids";

/// A saved account as the daemon lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub id: String,
    pub email: String,
    pub plan: Option<String>,
}

/// The accounts in `file`; empty when there are none or it isn't ours.
pub fn read_accounts(file: &DocFile) -> Vec<Saved> {
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
pub fn write_accounts(file: &DocFile, accounts: &[Saved]) -> bool {
    let list: Vec<Value> = accounts
        .iter()
        .map(|a| json!({"id": a.id, "email": a.email, "plan": a.plan}))
        .collect();
    write_json(file, "accounts", &json!({"version": 1, "accounts": list}))
}

/// The ids in `file` (Claude Desktop's accounts it has cookies for).
pub fn read_ids(file: &DocFile) -> Vec<String> {
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

pub fn write_ids(file: &DocFile, ids: &[String]) -> bool {
    write_json(file, "ids", &json!({"version": 1, "ids": ids}))
}

fn read_json(file: &DocFile) -> Option<Value> {
    serde_json::from_str(&file.read().ok()??).ok()
}

/// An object with `key` as a list: what an old file has to be to move in.
fn ours(text: &str, key: &str) -> bool {
    serde_json::from_str::<Value>(text).is_ok_and(|v| v.get(key).is_some_and(Value::is_array))
}

fn write_json(file: &DocFile, key: &str, value: &Value) -> bool {
    let Ok(text) = serde_json::to_string_pretty(value) else {
        return false;
    };
    file.put(|old| ours(old, key), &text).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::testutil::TempDir;

    fn list() -> Vec<Saved> {
        vec![
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
        ]
    }

    #[test]
    fn accounts_round_trip_through_the_database_and_a_foreign_old_file_reads_as_empty() {
        let tmp = TempDir::new("book-roundtrip");
        let data = tmp.path("data");
        let file = DocFile::new(
            &data,
            CODEX_DOC,
            tmp.path("data/daemon/accounts/codex.json"),
        );
        assert!(read_accounts(&file).is_empty());
        assert!(!crate::store::exists(&data), "a read makes no database");
        assert!(write_accounts(&file, &list()));
        assert_eq!(read_accounts(&file), list());
        assert!(crate::store::doc(&data, CODEX_DOC).unwrap().is_some());

        let foreign = tmp.write("data/daemon/accounts/antigravity.json", "[1,2]");
        let other = DocFile::new(&data, ANTIGRAVITY_DOC, foreign.clone());
        assert!(read_accounts(&other).is_empty());
        assert!(write_accounts(&other, &list()));
        assert_eq!(read_accounts(&other), list());
        assert!(foreign.exists(), "a file that isn't ours is left alone");

        let ids = DocFile::new(&data, IDS_DOC, tmp.path("data/daemon/accounts/ids.json"));
        assert!(write_ids(&ids, &["a".into(), "b".into()]));
        assert_eq!(read_ids(&ids), vec!["a", "b"]);
    }

    #[test]
    fn the_old_file_is_read_until_a_change_moves_it_in() {
        let tmp = TempDir::new("book-import");
        let data = tmp.path("data");
        let old = tmp.write(
            "data/daemon/accounts/codex.json",
            r#"{"version":1,"accounts":[{"id":"u:w","email":"a@example.com","plan":"Plus"}]}"#,
        );
        let file = DocFile::new(&data, CODEX_DOC, old.clone());
        assert_eq!(read_accounts(&file), list()[..1].to_vec());
        assert!(old.exists() && !crate::store::exists(&data));

        let mut more = read_accounts(&file);
        more.push(list()[1].clone());
        assert!(write_accounts(&file, &more));
        assert!(!old.exists(), "moved in");
        assert_eq!(read_accounts(&file), list());
    }
}
