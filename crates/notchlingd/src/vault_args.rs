use notchling_engine::vault::{default_vars, Kind, VaultEntry, VaultValue};
use notchling_local_api::ApiError;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Deny,
    Once,
    Always,
}

impl Decision {
    pub(crate) fn parse(text: &str) -> Option<Decision> {
        match text {
            "deny" => Some(Decision::Deny),
            "once" => Some(Decision::Once),
            "always" => Some(Decision::Always),
            _ => None,
        }
    }
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Decision::Deny => "deny",
            Decision::Once => "once",
            Decision::Always => "always",
        }
    }
}

pub(crate) fn text(args: &Value, key: &str, max: usize) -> Result<Option<String>, ApiError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.len() <= max => Ok(Some(s.clone())),
        Some(Value::String(_)) => Err(ApiError::bad_request(format!("{key} is over {max} bytes"))),
        Some(_) => Err(ApiError::bad_request(format!("{key} must be a string"))),
    }
}

pub(crate) fn entry_from(value: Option<&Value>) -> Result<VaultEntry, ApiError> {
    let bad = || ApiError::bad_request("entry must be {name, kind, …} as vault.json holds it");
    let mut entry: VaultEntry =
        serde_json::from_value(value.cloned().ok_or_else(bad)?).map_err(|_| bad())?;
    let (user, secret) = default_vars(&entry.name, entry.kind);
    if entry.secret_var.is_empty() {
        entry.secret_var = secret;
    }
    if entry.kind == Kind::Login && entry.user_var.is_empty() {
        entry.user_var = user;
    }
    Ok(entry)
}

pub(crate) fn value_from(value: Option<&Value>) -> Result<Option<VaultValue>, ApiError> {
    let bad = || ApiError::bad_request("value must be {username?: string, secret: string}");
    let Some(v) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let field = |key: &str| match v.get(key) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(bad()),
    };
    Ok(Some(VaultValue {
        username: field("username")?,
        secret: field("secret")?,
    }))
}

pub(crate) fn entry_json(e: &VaultEntry) -> Value {
    serde_json::to_value(e).unwrap_or(Value::Null)
}
