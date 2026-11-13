use crate::accounts::http::{HttpClient, HttpRequest, CLIENT_ID, TOKEN_URL};
use crate::accounts::{Login, Secret};
use base64::Engine;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub const AUTHORIZE_URL: &str = "https://claude.com/cai/oauth/authorize";
pub const SUCCESS_URL: &str = "https://platform.claude.com/oauth/code/success?app=claude-code";
pub const SCOPES: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload user:plugins";

pub struct SignIn {
    verifier: Secret,
    state: Secret,
    pub port: u16,
}

impl std::fmt::Debug for SignIn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SignIn {{ port: {} }}", self.port)
    }
}

pub fn random_seed() -> Option<[u8; 64]> {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 64];
    ring::rand::SystemRandom::new().fill(&mut bytes).ok()?;
    Some(bytes)
}

pub fn base64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn challenge(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

fn escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn unescape(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    Code(Secret),
    NotOurs,
}

impl SignIn {
    pub fn new(random: [u8; 64], port: u16) -> SignIn {
        SignIn {
            verifier: Secret::new(base64url(&random[..32])),
            state: Secret::new(base64url(&random[32..])),
            port,
        }
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://localhost:{}/callback", self.port)
    }

    pub fn page_url(&self) -> String {
        let params = [
            ("code", "true".to_string()),
            ("client_id", CLIENT_ID.to_string()),
            ("response_type", "code".to_string()),
            ("redirect_uri", self.redirect_uri()),
            ("scope", SCOPES.to_string()),
            ("code_challenge", challenge(self.verifier.expose())),
            ("code_challenge_method", "S256".to_string()),
            ("state", self.state.expose().to_string()),
        ];
        let query: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{k}={}", escape(v)))
            .collect();
        format!("{AUTHORIZE_URL}?{}", query.join("&"))
    }

    pub fn callback(&self, request: &str) -> Callback {
        let target = request.split(' ').nth(1).unwrap_or("");
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        if path != "/callback" {
            return Callback::NotOurs;
        }
        let mut code = None;
        let mut state = None;
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            match k {
                "code" if code.is_none() => code = unescape(v),
                "state" if state.is_none() => state = unescape(v),
                _ => {}
            }
        }
        match (code, state) {
            (Some(code), Some(state)) if !code.is_empty() && state == self.state.expose() => {
                Callback::Code(Secret::new(code))
            }
            _ => Callback::NotOurs,
        }
    }

    pub fn exchange_body(&self, code: &Secret) -> Value {
        json!({
            "grant_type": "authorization_code",
            "code": code.expose(),
            "redirect_uri": self.redirect_uri(),
            "client_id": CLIENT_ID,
            "code_verifier": self.verifier.expose(),
            "state": self.state.expose(),
        })
    }

    pub fn exchange(
        &self,
        http: &dyn HttpClient,
        code: &Secret,
        now_ms: i64,
    ) -> Result<Login, String> {
        let answer = http.send(&HttpRequest::post(TOKEN_URL, self.exchange_body(code)));
        if answer.status == 0 {
            return Err("Couldn't reach Anthropic to finish signing in.".into());
        }
        let json = answer
            .json_ok()
            .ok_or_else(|| format!("Sign-in was refused ({}). Try again.", answer.status))?;
        login_from_token_answer(&json, now_ms)
            .ok_or_else(|| format!("Sign-in was refused ({}). Try again.", answer.status))
    }
}

pub fn login_from_token_answer(json: &Map<String, Value>, now_ms: i64) -> Option<Login> {
    let access = json.get("access_token")?.as_str()?;
    let refresh = json.get("refresh_token")?.as_str()?;
    let now = now_ms as f64 / 1000.0;
    let expires_in = json
        .get("expires_in")
        .and_then(Value::as_f64)
        .unwrap_or(3600.0);
    let scopes: Vec<&str> = json
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or(SCOPES)
        .split(' ')
        .filter(|s| !s.is_empty())
        .collect();
    let mut oauth = Map::new();
    oauth.insert("accessToken".into(), json!(access));
    oauth.insert("refreshToken".into(), json!(refresh));
    oauth.insert(
        "expiresAt".into(),
        json!(((now + expires_in) * 1000.0) as i64),
    );
    oauth.insert("scopes".into(), json!(scopes));
    if let Some(s) = json.get("refresh_token_expires_in").and_then(Value::as_f64) {
        oauth.insert(
            "refreshTokenExpiresAt".into(),
            json!(((now + s) * 1000.0) as i64),
        );
    }
    Some(Login(oauth))
}

pub fn reply(found: bool) -> String {
    if found {
        format!("HTTP/1.1 302 Found\r\nContent-Length: 0\r\nConnection: close\r\nLocation: {SUCCESS_URL}\r\n\r\n")
    } else {
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
    }
}

#[cfg(test)]
#[path = "signin_tests.rs"]
mod tests;
