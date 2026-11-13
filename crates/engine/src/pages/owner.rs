use super::content::page_title;
use super::library::PageLibrary;
use super::record::{read_bounded, RecordContext};
use crate::search::artifact_id_in;
use serde_json::{json, Map, Value};
use std::path::Path;

pub const API_HOST: &str = "https://api.anthropic.com";
pub const BUDGET_SECS: f64 = 50.0;
pub const MINIMUM_POST_SECS: f64 = 8.0;
pub const PERM_TIMEOUT_SECS: f64 = 20.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done(String),
    Failed(String),
    Refused(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HttpRequest {
    pub method: Method,
    pub path: String,
    pub token: String,
    pub body: Option<Value>,
    pub timeout_secs: f64,
}

impl HttpRequest {
    pub fn headers(&self) -> Vec<(&'static str, String)> {
        let mut headers = vec![
            ("Authorization", format!("Bearer {}", self.token)),
            ("anthropic-beta", "oauth-2025-04-20".to_string()),
            ("Accept", "application/json".to_string()),
            ("X-Frame-CP", "go".to_string()),
            ("X-Frame-Surface", "code".to_string()),
            ("X-Frame-Platform", "cli".to_string()),
            ("User-Agent", "Notchling".to_string()),
        ];
        if self.body.is_some() {
            headers.push(("Content-Type", "application/json".to_string()));
        }
        headers
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    fn json(&self) -> Option<Map<String, Value>> {
        match serde_json::from_str::<Value>(&self.body) {
            Ok(Value::Object(map)) => Some(map),
            _ => None,
        }
    }

    fn message(&self) -> String {
        let text: String = self.body.chars().take(300).collect();
        let text = text.trim();
        if text.is_empty() || self.status == 200 {
            String::new()
        } else {
            format!(": {text}")
        }
    }
}

pub trait HttpClient {
    fn send(&self, request: &HttpRequest) -> HttpResponse;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerToken {
    pub email: String,
    pub token: String,
    pub org: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenError {
    Unavailable(String),
    NoAccount,
}

pub trait TokenSource {
    fn token_for(&self, email: &str) -> Result<OwnerToken, TokenError>;
}

pub struct UnavailableTokens;

impl TokenSource for UnavailableTokens {
    fn token_for(&self, _email: &str) -> Result<OwnerToken, TokenError> {
        Err(TokenError::Unavailable("accounts arrive in M6".to_string()))
    }
}

pub trait Clock {
    fn now(&self) -> f64;
}

pub struct OwnerEnv<'a> {
    pub tokens: &'a dyn TokenSource,
    pub http: &'a dyn HttpClient,
    pub clock: &'a dyn Clock,
    pub active_email: &'a str,
    pub record: RecordContext<'a>,
}

const UNSUPPORTED: [&str; 8] = [
    "asset",
    "files",
    "file_paths",
    "root",
    "type_url",
    "from_url",
    "capabilities",
    "contract",
];

pub fn run(lib: &PageLibrary, event: &Value, env: &OwnerEnv<'_>) -> Option<Outcome> {
    let empty = Map::new();
    let input = event
        .get("tool_input")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let action = input.get("action").and_then(Value::as_str);
    if !(action.is_none() || action == Some("publish")) {
        return None;
    }
    let id = artifact_id_in(input.get("url")?.as_str()?).filter(|id| id.chars().count() == 22)?;
    let path = input.get("file_path")?.as_str()?;
    if Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .as_deref()
        != Some("html")
        || UNSUPPORTED.iter().any(|k| input.contains_key(*k))
    {
        return None;
    }
    let page = lib
        .entries()
        .into_iter()
        .find(|e| e.artifact_id() == Some(id.as_str()))
        .filter(|e| !e.account.is_empty())?;
    let owner = env.tokens.token_for(&page.account).ok()?;
    if !owner.email.eq_ignore_ascii_case(&page.account) {
        return Some(Outcome::Failed(format!(
            "Notchling couldn't publish this as {}, the page's owner: the saved login it found belongs to another account, so nothing was sent.",
            page.account
        )));
    }

    let cwd = event.get("cwd").and_then(Value::as_str).unwrap_or("");
    let base = if cwd.is_empty() {
        env.record.home.to_path_buf()
    } else {
        std::path::PathBuf::from(cwd)
    };
    let source = super::record::join_normalized(&base, path);
    let mut text = String::from_utf8(read_bounded(&source)?).ok()?;
    let edits: Vec<&Value> = input
        .get("edits")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    for edit in &edits {
        let old = edit.get("old_str")?.as_str()?;
        let new = edit.get("new_str")?.as_str()?;
        if old.is_empty() || text.matches(old).count() != 1 {
            return None;
        }
        text = text.replacen(old, new, 1);
    }

    let slug = uuid_from_short_id(&id);
    let deadline = env.clock.now() + BUDGET_SECS;
    let perm = env.http.send(&HttpRequest {
        method: Method::Get,
        path: format!("/api/frame/perm/{slug}?org={}", percent_encode(&owner.org)),
        token: owner.token.clone(),
        body: None,
        timeout_secs: PERM_TIMEOUT_SECS.min(deadline - env.clock.now()),
    });
    let live = (perm.status == 200)
        .then(|| perm.json())
        .flatten()
        .and_then(|j| j.get("live").and_then(Value::as_str).map(str::to_string));
    let Some(live) = live else {
        return Some(Outcome::Failed(format!(
            "Notchling couldn't publish this as {}, the page's owner: reading the page failed (HTTP {}{}).",
            page.account,
            perm.status,
            perm.message()
        )));
    };
    let force = input.get("force").and_then(Value::as_bool) == Some(true);
    if let Some(refusal) = stale_refusal(&page.url, &page.version, &live, force) {
        return Some(Outcome::Refused(refusal));
    }
    let body = publish_body(&slug, &live, &text, &page.title, input);
    let left = deadline - env.clock.now();
    if left < MINIMUM_POST_SECS {
        return Some(Outcome::Failed(format!(
            "Notchling couldn't publish this as {}, the page's owner: claude.ai answered too slowly to finish within the hook's time limit. Nothing was published; try again.",
            page.account
        )));
    }
    let result = env.http.send(&HttpRequest {
        method: Method::Post,
        path: "/api/frame/deploy/direct".to_string(),
        token: owner.token,
        body: Some(body),
        timeout_secs: left,
    });
    if result.status == 0 {
        return Some(Outcome::Failed(format!(
            "Notchling couldn't confirm the publish as {}, the page's owner{}. It may still have gone through: read the page before publishing again.",
            page.account,
            result.message()
        )));
    }
    let version = (result.status == 200)
        .then(|| result.json())
        .flatten()
        .and_then(|j| j.get("version").and_then(Value::as_str).map(str::to_string));
    let Some(version) = version else {
        return Some(Outcome::Failed(format!(
            "Notchling couldn't publish this as {}, the page's owner (HTTP {}{}).",
            page.account,
            result.status,
            result.message()
        )));
    };
    if !edits.is_empty() {
        let _ = std::fs::write(&source, &text);
    }

    let mut saved = event.clone();
    saved["hook_event_name"] = json!("PostToolUse");
    saved["tool_response"] = json!(format!(
        "Published {path} at {} (version {version})",
        page.url
    ));
    let _ = lib.record(&saved, &env.record);

    let signed_in = if env.active_email.is_empty() {
        String::new()
    } else {
        format!(
            " (Claude Code is signed in as {}, which can't publish to it)",
            env.active_email
        )
    };
    let same = if version == live {
        format!(". claude.ai already had exactly this content (version {version})")
    } else {
        format!(". New version {version}, over {live}")
    };
    Some(Outcome::Done(format!(
        "Done, not an error: Notchling published {path} to {} as {}, the page's owner{signed_in}{same}{} Don't publish it again. Notchling's local copy is updated.",
        page.url,
        page.account,
        if edits.is_empty() {
            ".".to_string()
        } else {
            "; the edits were applied to the file too.".to_string()
        }
    )))
}

pub fn stale_refusal(url: &str, recorded: &str, live: &str, force: bool) -> Option<String> {
    if recorded.is_empty() || live == recorded || force {
        return None;
    }
    Some(format!(
        "Not published: {url} has a newer version on claude.ai ({live}) than the one last published \
from this Mac ({recorded}), so publishing this file would overwrite it. Notchling's local copy is the older \
version. Ask the user whether to overwrite it; if so, publish again with force: true."
    ))
}

pub fn publish_body(
    slug: &str,
    base_version: &str,
    content: &str,
    fallback_title: &str,
    input: &Map<String, Value>,
) -> Value {
    let mut body = Map::new();
    body.insert("slug".into(), json!(slug));
    body.insert("baseVersion".into(), json!(base_version));
    body.insert("content".into(), json!(content));
    body.insert(
        "title".into(),
        json!(page_title(content.as_bytes(), "html", fallback_title)),
    );
    for key in ["description", "label"] {
        if let Some(v) = input.get(key).and_then(Value::as_str) {
            body.insert(key.into(), json!(v));
        }
    }
    if input.get("force").and_then(Value::as_bool) == Some(true) {
        body.insert("force".into(), json!(true));
    }
    Value::Object(body)
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

pub fn uuid_from_short_id(id: &str) -> String {
    let mut bytes = [0u8; 16];
    for ch in id.bytes() {
        let Some(digit) = ALPHABET.iter().position(|&a| a == ch) else {
            return id.to_string();
        };
        let mut carry = digit;
        for byte in bytes.iter_mut().rev() {
            carry += usize::from(*byte) * 58;
            *byte = (carry & 0xff) as u8;
            carry >>= 8;
        }
    }
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn short_id_from_uuid(uuid: &str) -> Option<String> {
    let hex: String = uuid.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 || uuid.len() != 36 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = Vec::with_capacity(16);
    for i in 0..16 {
        bytes.push(u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?);
    }
    let mut digits: Vec<u8> = Vec::new();
    let mut number = bytes;
    while number.iter().any(|&b| b != 0) {
        let mut rem = 0usize;
        for byte in number.iter_mut() {
            let cur = rem * 256 + usize::from(*byte);
            *byte = (cur / 58) as u8;
            rem = cur % 58;
        }
        digits.push(rem as u8);
    }
    while digits.len() < 22 {
        digits.push(0);
    }
    Some(
        digits
            .iter()
            .rev()
            .map(|&d| char::from(ALPHABET[usize::from(d)]))
            .collect(),
    )
}
