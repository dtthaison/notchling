//! Claude's model list as Anthropic reports it (`GET /v1/models`), read with the active login's
//! token. It only names models: the window is Claude Code's own (the data file's), not the API's
//! `max_input_tokens`, which differs.

use serde_json::Value;

use super::LiveModel;
use crate::accounts::http::{HttpClient, HttpRequest};
use crate::accounts::Secret;

/// The whole list in one page: `limit=1000` is far past the list's length, so `has_more` is
/// ignored on purpose.
pub const MODELS_URL: &str = "https://api.anthropic.com/v1/models?limit=1000";

/// The API's version header; the OAuth beta header is already sent by the client.
const VERSION: &str = "2023-06-01";

/// Why a fetch got no list. It names only the status or the shape of the answer, never its body
/// or the token, so it can be logged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchError {
    NoAnswer,
    Status(u16),
    NotAList,
}

impl FetchError {
    /// 400, 403 or 404: the endpoint won't serve this login the list, and asking again soon with
    /// the same token would be refused too. A 401 (a token that just expired or was replaced) and
    /// a 429 (asked too often) are not: they pass with time.
    pub fn refused(self) -> bool {
        matches!(self, FetchError::Status(400 | 403 | 404))
    }
}

/// A fingerprint of a token (SHA-256), to tell whether the live login changed without keeping
/// the token itself.
pub fn token_fingerprint(token: &Secret) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(token.expose().as_bytes()).into()
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::NoAnswer => write!(f, "no answer"),
            FetchError::Status(status) => write!(f, "status {status}"),
            FetchError::NotAList => write!(f, "answer is not a model list"),
        }
    }
}

/// The models Anthropic lists for this login. Nothing is retried: the caller keeps its last list.
pub fn fetch(http: &dyn HttpClient, token: &Secret) -> Result<Vec<LiveModel>, FetchError> {
    let request = HttpRequest::get(MODELS_URL, token).with_header("anthropic-version", VERSION);
    let answer = http.send(&request);
    match answer.status {
        200 => {}
        0 => return Err(FetchError::NoAnswer),
        status => return Err(FetchError::Status(status)),
    }
    match serde_json::from_slice::<Value>(&answer.body) {
        Ok(Value::Object(map)) if map.get("data").is_some_and(Value::is_array) => {
            Ok(parse(&answer.body))
        }
        _ => Err(FetchError::NotAList),
    }
}

/// `data[].id` and `display_name`; an entry with no id is skipped, and garbage is no models.
pub fn parse(body: &[u8]) -> Vec<LiveModel> {
    let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let Some(Value::Array(data)) = map.get("data") else {
        return Vec::new();
    };
    data.iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str().filter(|id| !id.is_empty())?;
            Some(LiveModel {
                id: id.to_string(),
                name: entry
                    .get("display_name")
                    .and_then(Value::as_str)
                    .filter(|n| !n.is_empty())
                    .map(str::to_string),
                ..LiveModel::default()
            })
        })
        .collect()
}
