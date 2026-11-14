use super::keys::{spec, Kind};
use notchling_events::kinds::settings::{SettingKey, SettingValue};
use notchling_events::{Count, Hex, Salt};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoggedSetting {
    pub key: SettingKey,
    pub key_hash: Option<Hex>,
    pub value_flag: Option<bool>,
    pub value_number: Option<Count>,
    pub value_choice: Option<SettingValue>,
    pub value_chars: Option<Count>,
    pub value_hash: Option<Hex>,
}

fn hashed(salt: &Salt, what: &str, text: &str) -> Hex {
    let mut hasher = Sha256::new();
    hasher.update(salt.0);
    hasher.update(what.as_bytes());
    hasher.update([0u8]);
    hasher.update(text.as_bytes());
    Hex::from_bytes(&hasher.finalize()[..8])
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn choice(text: &str) -> Option<SettingValue> {
    SettingValue::from_name(text).filter(|v| v.name() == text)
}

pub fn log_value(key: &str, value: &Value, salt: &Salt) -> LoggedSetting {
    let mut out = LoggedSetting {
        key: SettingKey::Unknown,
        key_hash: None,
        value_flag: None,
        value_number: None,
        value_choice: None,
        value_chars: None,
        value_hash: None,
    };
    let named = SettingKey::from_name(key).filter(|k| k.name() == key);
    let Some((known, spec)) = named.zip(spec(key)) else {
        out.key_hash = Some(hashed(salt, "setting-key", key));
        let text = text_of(value);
        out.value_chars = Some(Count(text.chars().count() as u64));
        out.value_hash = Some(hashed(salt, "setting-value", &text));
        return out;
    };
    out.key = known;
    match (spec.kind, value) {
        (Kind::Bool, Value::Bool(flag)) => out.value_flag = Some(*flag),
        (Kind::Int, Value::Number(n)) | (Kind::Double, Value::Number(n)) => {
            let whole = n
                .as_u64()
                .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64));
            out.value_number = whole.map(Count);
        }
        (Kind::String, Value::String(text)) if !spec.allowed.is_empty() => {
            if spec.allowed.contains(&text.as_str()) {
                out.value_choice = choice(text);
            }
            if out.value_choice.is_none() {
                out.value_chars = Some(Count(text.chars().count() as u64));
                out.value_hash = Some(hashed(salt, "setting-value", text));
            }
        }
        (_, other) => {
            let text = text_of(other);
            out.value_chars = Some(Count(text.chars().count() as u64));
            out.value_hash = Some(hashed(salt, "setting-value", &text));
        }
    }
    out
}

#[cfg(test)]
#[path = "keys_log_tests.rs"]
mod tests;
