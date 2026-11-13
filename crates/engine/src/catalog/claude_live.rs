use serde_json::Value;

use super::LiveModel;
use crate::accounts::http::{HttpClient, HttpRequest};
use crate::accounts::Secret;

pub const MODELS_URL: &str = "https://api.anthropic.com/v1/models?limit=1000";

const VERSION: &str = "2023-06-01";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchError {
    NoAnswer,
    Status(u16),
    NotAList,
}

impl FetchError {
    pub fn refused(self) -> bool {
        matches!(self, FetchError::Status(400 | 403 | 404))
    }
}

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
