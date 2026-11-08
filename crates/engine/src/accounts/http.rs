//! The account endpoints over HTTPS: the profile check (`GET /api/oauth/profile`) and the token
//! endpoint (sign-in and renewing a saved login that isn't live anywhere). Behind [`HttpClient`]
//! so tests never reach the network; [`LiveHttp`] is the real one, which talks only to the two
//! hosts Claude's login uses.

use super::Secret;
use serde_json::{Map, Value};
use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

/// Claude Code's OAuth client id (`AccountStore.clientID`).
pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
/// The account behind a token.
pub const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
/// Codes and refresh tokens are exchanged here.
pub const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
/// The only hosts [`LiveHttp`] will call.
pub const HOSTS: [&str; 2] = ["api.anthropic.com", "platform.claude.com"];
/// Each request's time, as the Swift app's `timeoutInterval`.
pub const TIMEOUT: Duration = Duration::from_secs(15);
const LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// One request. Its `Debug` hides the bearer token and the body (a token exchange carries a code
/// or a refresh token).
#[derive(Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub bearer: Option<Secret>,
    /// Sent as JSON.
    pub body: Option<Value>,
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HttpRequest {{ {:?} {} }}", self.method, self.url)
    }
}

impl HttpRequest {
    /// `GET url` with `Authorization: Bearer <token>` and the OAuth beta header.
    pub fn get(url: &str, token: &Secret) -> HttpRequest {
        HttpRequest {
            method: Method::Get,
            url: url.into(),
            bearer: Some(token.clone()),
            body: None,
        }
    }

    pub fn post(url: &str, body: Value) -> HttpRequest {
        HttpRequest {
            method: Method::Post,
            url: url.into(),
            bearer: None,
            body: Some(body),
        }
    }
}

/// An answer. Status 0 means none came. Its `Debug` hides the body (a token answer holds tokens).
#[derive(Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "HttpResponse {{ status: {}, {} bytes }}",
            self.status,
            self.body.len()
        )
    }
}

impl HttpResponse {
    /// The body as a JSON object, for a 200 only.
    pub fn json_ok(&self) -> Option<Map<String, Value>> {
        if self.status != 200 {
            return None;
        }
        match serde_json::from_slice::<Value>(&self.body) {
            Ok(Value::Object(map)) => Some(map),
            _ => None,
        }
    }
}

/// The network.
pub trait HttpClient: Send + Sync {
    fn send(&self, request: &HttpRequest) -> HttpResponse;
}

/// The real client: blocking HTTPS through rustls with the OS's roots, to [`HOSTS`] only.
pub struct LiveHttp;

impl HttpClient for LiveHttp {
    fn send(&self, request: &HttpRequest) -> HttpResponse {
        send(request).unwrap_or(HttpResponse {
            status: 0,
            body: Vec::new(),
        })
    }
}

/// A client that sends nothing (a fixture root).
pub struct NoNetwork;

impl HttpClient for NoNetwork {
    fn send(&self, _request: &HttpRequest) -> HttpResponse {
        HttpResponse {
            status: 0,
            body: Vec::new(),
        }
    }
}

/// `(host, path)` of an https URL on one of [`HOSTS`].
pub fn split_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    HOSTS.contains(&host).then_some((host, path))
}

/// The request's bytes: head and JSON body.
pub fn request_bytes(request: &HttpRequest, host: &str, path: &str) -> Vec<u8> {
    let method = match request.method {
        Method::Get => "GET",
        Method::Post => "POST",
    };
    let body = request
        .body
        .as_ref()
        .map(|b| b.to_string().into_bytes())
        .unwrap_or_default();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nConnection: close\r\nUser-Agent: Notchling\r\nanthropic-beta: oauth-2025-04-20\r\n"
    );
    if let Some(token) = &request.bearer {
        head.push_str(&format!("Authorization: Bearer {}\r\n", token.expose()));
    }
    if request.body.is_some() {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(&body);
    bytes
}

fn send(request: &HttpRequest) -> Result<HttpResponse, String> {
    let (host, path) = split_url(&request.url).ok_or("not an account endpoint")?;
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name =
        rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
    let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    let addr = (host, 443)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let sock = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    sock.set_write_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    let mut tls = rustls::StreamOwned::new(conn, sock);
    tls.write_all(&request_bytes(request, host, path))
        .map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        match tls.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        }
        if data.len() > LIMIT {
            return Err("answer too large".into());
        }
    }
    parse_response(&data)
}

/// Status and body (plain or chunked) of an HTTP/1.1 answer.
pub fn parse_response(data: &[u8]) -> Result<HttpResponse, String> {
    let end = data
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no head")?;
    let head = std::str::from_utf8(&data[..end]).map_err(|_| "head not text")?;
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or("no status")?;
    let chunked = head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let body = &data[end + 4..];
    let body = if chunked {
        crate::bridge::net::fetch::parse_response(
            &[
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
                body,
            ]
            .concat(),
        )?
    } else {
        body.to_vec()
    };
    Ok(HttpResponse { status, body })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_the_login_hosts_are_called() {
        assert_eq!(
            split_url(PROFILE_URL),
            Some(("api.anthropic.com", "/api/oauth/profile"))
        );
        assert_eq!(
            split_url(TOKEN_URL).map(|s| s.0),
            Some("platform.claude.com")
        );
        assert_eq!(split_url("http://api.anthropic.com/x"), None);
        assert_eq!(split_url("https://example.com/x"), None);
        assert_eq!(split_url("https://api.anthropic.com.evil.test/x"), None);
        let sent = LiveHttp.send(&HttpRequest::get("https://example.com/", &Secret::new("t")));
        assert_eq!(sent.status, 0, "refused before any connection");
    }

    #[test]
    fn a_request_carries_the_bearer_and_json_body_but_never_prints_them() {
        let get = HttpRequest::get(PROFILE_URL, &Secret::new("test-access-token"));
        let bytes = String::from_utf8(request_bytes(
            &get,
            "api.anthropic.com",
            "/api/oauth/profile",
        ))
        .unwrap();
        assert!(bytes.starts_with("GET /api/oauth/profile HTTP/1.1\r\n"));
        assert!(bytes.contains("Authorization: Bearer test-access-token\r\n"));
        assert!(bytes.contains("anthropic-beta: oauth-2025-04-20\r\n"));
        assert!(!format!("{get:?}").contains("test-access-token"));
        let post = HttpRequest::post(TOKEN_URL, json!({"refresh_token": "test-refresh-token"}));
        let bytes = String::from_utf8(request_bytes(
            &post,
            "platform.claude.com",
            "/v1/oauth/token",
        ))
        .unwrap();
        assert!(bytes.contains("Content-Type: application/json\r\n"));
        assert!(bytes.ends_with("{\"refresh_token\":\"test-refresh-token\"}"));
        assert!(!format!("{post:?}").contains("test-refresh-token"));
    }

    #[test]
    fn answers_keep_their_status() {
        let ok = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!(ok.status, 200);
        assert!(ok.json_ok().is_some());
        let refused = parse_response(b"HTTP/1.1 401 Unauthorized\r\n\r\n{\"error\":1}").unwrap();
        assert_eq!(refused.status, 401);
        assert!(refused.json_ok().is_none(), "only a 200 is read");
        let chunked = parse_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(chunked.body, b"{}");
        assert!(parse_response(b"junk").is_err());
    }
}
