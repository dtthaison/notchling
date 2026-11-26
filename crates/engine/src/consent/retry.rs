use super::model::{Consent, New, Refusal, State};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const RETRY_KEYS: [&str; 3] = ["retryOf", "priorNote", "priorSummary"];
const SENT_KEYS: [&str; 2] = ["message", "question"];
pub const SENT_HASH: &str = "sentHash";

pub const NO_OLD: &str = "No request with that id to ask again.";
pub const NOT_NOTED: &str = "That request wasn't denied with a note, so it can't be asked again.";
pub const OTHER_ASKER: &str = "That request was made by another caller.";
pub const OTHER_KIND: &str = "That request was about something else.";
pub const ALREADY: &str = "That request was already asked again.";
pub const UNCHANGED: &str = "Change the request or its reason before asking again.";
pub const NOT_AN_OBJECT: &str = "This request can't be asked again.";

pub fn bare(change: &Value) -> Value {
    let mut change = change.clone();
    if let Some(map) = change.as_object_mut() {
        for key in RETRY_KEYS.into_iter().chain(SENT_KEYS).chain([SENT_HASH]) {
            map.remove(key);
        }
    }
    change
}

pub fn sent_hash(change: &Value) -> Option<String> {
    let sent: Vec<(&str, &Value)> = SENT_KEYS
        .into_iter()
        .filter_map(|key| change.get(key).map(|v| (key, v)))
        .collect();
    if sent.is_empty() {
        return None;
    }
    let text = json!(sent).to_string();
    Some(format!("{:x}", Sha256::digest(text.as_bytes())))
}

pub fn unretried(change: &Value) -> Value {
    let mut change = change.clone();
    if let Some(map) = change.as_object_mut() {
        for key in RETRY_KEYS {
            map.remove(key);
        }
    }
    change
}

pub fn retry_of(change: &Value) -> Option<&str> {
    change.get("retryOf").and_then(Value::as_str)
}

pub fn check_retry(old: &Consent, new: &New) -> Result<Value, Refusal> {
    let Some(note) = old
        .note
        .as_ref()
        .filter(|_| old.state == State::Denied)
        .filter(|note| !note.is_empty())
    else {
        return Err(Refusal::RetryRefused(NOT_NOTED));
    };
    if old.bucket() != new.bucket() {
        return Err(Refusal::RetryRefused(OTHER_ASKER));
    }
    if old.kind != new.kind {
        return Err(Refusal::RetryRefused(OTHER_KIND));
    }
    let old_sent = old.change.get(SENT_HASH).and_then(Value::as_str);
    let same = bare(&old.change) == bare(&new.change)
        && old_sent.map(str::to_string) == sent_hash(&new.change)
        && old.target == new.target
        && old.reason == new.reason;
    if same {
        return Err(Refusal::RetryRefused(UNCHANGED));
    }
    let mut change = bare(&new.change);
    let Some(map) = change.as_object_mut() else {
        return Err(Refusal::RetryRefused(NOT_AN_OBJECT));
    };
    if let Some(message) = new.change.get("message") {
        map.insert("message".into(), message.clone());
    }
    if let Some(question) = new.change.get("question") {
        map.insert("question".into(), question.clone());
    }
    map.insert("retryOf".into(), json!(old.id));
    map.insert("priorNote".into(), json!(note));
    map.insert(
        "priorSummary".into(),
        json!(format!("{} {}", old.kind, old.target)),
    );
    Ok(change)
}
