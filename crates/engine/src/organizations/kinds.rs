use crate::accounts::book::{ANTIGRAVITY_DOC, CODEX_DOC};
use crate::accounts::claude::LIST_DOC;
use crate::vault::list::INDEX_DOC;
use serde_json::Value;

pub const FIELD: &str = "organization";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Claude,
    Codex,
    Antigravity,
    Vault,
}

pub const KINDS: [Kind; 4] = [Kind::Claude, Kind::Codex, Kind::Antigravity, Kind::Vault];

impl Kind {
    pub fn doc(self) -> &'static str {
        match self {
            Kind::Claude => LIST_DOC,
            Kind::Codex => CODEX_DOC,
            Kind::Antigravity => ANTIGRAVITY_DOC,
            Kind::Vault => INDEX_DOC,
        }
    }

    pub fn is_account(self) -> bool {
        self != Kind::Vault
    }

    pub fn shadow_key(self, key: &str) -> String {
        match self {
            Kind::Claude => format!("claude:{key}"),
            Kind::Codex => format!("codex:{key}"),
            Kind::Antigravity => format!("antigravity:{key}"),
            Kind::Vault => key.to_lowercase(),
        }
    }

    pub fn normal(self, key: &str) -> String {
        match self {
            Kind::Vault => key.to_lowercase(),
            _ => key.to_owned(),
        }
    }

    pub fn entries(self, root: &Value) -> Option<&Vec<Value>> {
        match self {
            Kind::Claude | Kind::Vault => root.as_array(),
            Kind::Codex | Kind::Antigravity => root.get("accounts")?.as_array(),
        }
    }

    pub fn entries_mut(self, root: &mut Value) -> Option<&mut Vec<Value>> {
        match self {
            Kind::Claude | Kind::Vault => root.as_array_mut(),
            Kind::Codex | Kind::Antigravity => root.get_mut("accounts")?.as_array_mut(),
        }
    }

    pub fn key_of(self, entry: &Value) -> Option<String> {
        let field = match self {
            Kind::Vault => "name",
            _ => "id",
        };
        let key = entry.get(field)?.as_str()?;
        if key.is_empty() {
            return None;
        }
        Some(match self {
            Kind::Vault => key.to_lowercase(),
            _ => key.to_owned(),
        })
    }
}

pub fn field_of(entry: &Value) -> Option<String> {
    entry
        .get(FIELD)?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub fn listing(kind: Kind, text: &str) -> Option<Vec<(String, Option<String>)>> {
    let root: Value = serde_json::from_str(text).ok()?;
    let entries = kind.entries(&root)?;
    Some(
        entries
            .iter()
            .filter_map(|e| Some((kind.key_of(e)?, field_of(e))))
            .collect(),
    )
}

pub fn render(old: &str, root: &Value) -> Result<String, String> {
    if old.contains('\n') {
        serde_json::to_string_pretty(root)
    } else {
        serde_json::to_string(root)
    }
    .map_err(|e| e.to_string())
}
