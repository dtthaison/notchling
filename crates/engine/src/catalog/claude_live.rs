//! Claude's model list as Anthropic reports it (`GET /v1/models`), read with the active login's
//! token. It only names models: the window is Claude Code's own (the data file's), not the API's
//! `max_input_tokens`, which differs.

use serde_json::Value;

use super::LiveModel;
use crate::accounts::http::{HttpClient, HttpRequest};
use crate::accounts::Secret;

/// The whole list in one page.
pub const MODELS_URL: &str = "https://api.anthropic.com/v1/models?limit=1000";

/// The API's version header; the OAuth beta header is already sent by the client.
const VERSION: &str = "2023-06-01";

/// The models Anthropic lists for this login. An error says only the status or the shape of the
/// answer, never its body or the token, and nothing is retried: the caller keeps its last list.
pub fn fetch(http: &dyn HttpClient, token: &Secret) -> Result<Vec<LiveModel>, String> {
    let request = HttpRequest::get(MODELS_URL, token).with_header("anthropic-version", VERSION);
    let answer = http.send(&request);
    match answer.status {
        200 => {}
        0 => return Err("no answer".into()),
        status => return Err(format!("status {status}")),
    }
    match serde_json::from_slice::<Value>(&answer.body) {
        Ok(Value::Object(map)) if map.get("data").is_some_and(Value::is_array) => {
            Ok(parse(&answer.body))
        }
        _ => Err("answer is not a model list".into()),
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
