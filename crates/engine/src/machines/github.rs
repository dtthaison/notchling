//! GitHub's API for the token checks (Swift `HTTPFetching`, `URLSessionFetcher`). The token goes
//! only in the Authorization header of a request to `https://api.github.com`; any other URL is
//! refused (status 0) before a socket opens. [`LiveGitHub`] follows no redirect: a 3xx comes back as
//! it is, so the header can never reach another host.

use crate::accounts::Secret;
use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const API_HOST: &str = "api.github.com";
/// The most of a body kept.
pub const BODY_CAP: usize = 1 << 20;
const TIMEOUT: Duration = Duration::from_secs(20);

/// An answer. Header names are lower case. Status 0 means none came. `Debug` hides the body.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct HttpAnswer {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpAnswer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HttpAnswer({}, {} bytes)", self.status, self.body.len())
    }
}

impl HttpAnswer {
    pub fn none() -> HttpAnswer {
        HttpAnswer::default()
    }
    pub fn new(status: u16, headers: &[(&str, &str)], body: &str) -> HttpAnswer {
        HttpAnswer {
            status,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v.to_string()))
                .collect(),
            body: body.as_bytes().to_vec(),
        }
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

/// `GET url` with `Authorization: Bearer <token>`.
pub trait GitHubHttp: Send + Sync {
    fn get(&self, url: &str, bearer: &Secret) -> HttpAnswer;
}

/// `https://api.github.com/…` and nothing else: no other host, scheme, port or user info.
pub fn allows(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if !authority.eq_ignore_ascii_case(API_HOST) || !path.starts_with('/') {
        return None;
    }
    if path.chars().any(|c| c.is_control() || c == ' ' || c == '#') {
        return None;
    }
    Some(path)
}

/// No network: every answer is status 0 (the vault is off, a render, a test that doesn't care).
pub struct NoGitHub;

impl GitHubHttp for NoGitHub {
    fn get(&self, _url: &str, _bearer: &Secret) -> HttpAnswer {
        HttpAnswer::none()
    }
}

/// Answers by URL and records each URL asked (never the token: tests read [`FakeGitHub::bearers`]
/// only as a count of which tokens were used).
#[derive(Default)]
pub struct FakeGitHub {
    answers: Mutex<BTreeMap<String, HttpAnswer>>,
    asked: Mutex<Vec<(String, String)>>,
}

impl FakeGitHub {
    pub fn new() -> FakeGitHub {
        FakeGitHub::default()
    }
    pub fn answer(&self, url: &str, a: HttpAnswer) -> &Self {
        lock(&self.answers).insert(url.into(), a);
        self
    }
    pub fn urls(&self) -> Vec<String> {
        lock(&self.asked).iter().map(|(u, _)| u.clone()).collect()
    }
    /// The tokens used, in order.
    pub fn bearers(&self) -> Vec<String> {
        lock(&self.asked).iter().map(|(_, b)| b.clone()).collect()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl GitHubHttp for FakeGitHub {
    fn get(&self, url: &str, bearer: &Secret) -> HttpAnswer {
        if allows(url).is_none() {
            return HttpAnswer::none();
        }
        lock(&self.asked).push((url.into(), bearer.expose().into()));
        lock(&self.answers).get(url).cloned().unwrap_or_default()
    }
}

/// The real client: blocking HTTPS through rustls with the OS's roots, no cookies, no cache, no
/// redirects.
pub struct LiveGitHub;

impl GitHubHttp for LiveGitHub {
    fn get(&self, url: &str, bearer: &Secret) -> HttpAnswer {
        let Some(path) = allows(url) else {
            return HttpAnswer::none();
        };
        fetch(path, bearer).unwrap_or_default()
    }
}

fn fetch(path: &str, bearer: &Secret) -> Result<HttpAnswer, String> {
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
        rustls::pki_types::ServerName::try_from(API_HOST.to_string()).map_err(|e| e.to_string())?;
    let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    let addr = (API_HOST, 443)
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
    let head = format!(
        "GET {path} HTTP/1.1\r\nHost: {API_HOST}\r\nAccept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\nConnection: close\r\nUser-Agent: Notchling\r\nAuthorization: Bearer {}\r\n\r\n",
        bearer.expose()
    );
    tls.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        match tls.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        }
        if data.len() > BODY_CAP + 64 * 1024 {
            break;
        }
    }
    parse(&data)
}

/// Status, lower-case headers and body (plain or chunked, cut at [`BODY_CAP`]).
pub fn parse(data: &[u8]) -> Result<HttpAnswer, String> {
    let end = data
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no head")?;
    let head = std::str::from_utf8(&data[..end]).map_err(|_| "head not text")?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or("no status")?;
    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let raw = &data[end + 4..];
    let chunked = headers
        .get("transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let mut body = if chunked { dechunk(raw) } else { raw.to_vec() };
    body.truncate(BODY_CAP);
    Ok(HttpAnswer {
        status,
        headers,
        body,
    })
}

/// A chunked body joined; a cut last chunk keeps what came.
fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(nl) = raw.windows(2).position(|w| w == b"\r\n") else {
            return out;
        };
        let size = std::str::from_utf8(&raw[..nl])
            .ok()
            .and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or("").trim(), 16).ok());
        let Some(size) = size else { return out };
        if size == 0 {
            return out;
        }
        raw = &raw[nl + 2..];
        let take = size.min(raw.len());
        out.extend_from_slice(&raw[..take]);
        if take < size || raw.len() < take + 2 {
            return out;
        }
        raw = &raw[take + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_api_github_com_over_https_is_allowed() {
        assert_eq!(
            allows("https://api.github.com/user/repos?per_page=100"),
            Some("/user/repos?per_page=100")
        );
        assert_eq!(allows("https://API.github.com/x"), Some("/x"));
        for bad in [
            "http://api.github.com/x",
            "https://api.github.com.evil.com/x",
            "https://api.github.com:8443/x",
            "https://user@api.github.com/x",
            "https://evil.com/api.github.com/x",
            "https://api.github.com/x y",
            "https://api.github.com/x\r\nHost: evil",
        ] {
            assert_eq!(allows(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_fake_never_answers_another_host() {
        let fake = FakeGitHub::new();
        fake.answer("https://evil.com/x", HttpAnswer::new(200, &[], "{}"));
        assert_eq!(
            fake.get("https://evil.com/x", &Secret::new("fake-token"))
                .status,
            0
        );
        assert!(fake.urls().is_empty());
    }

    #[test]
    fn answers_keep_lower_case_headers_and_join_chunks() {
        let a = parse(b"HTTP/1.1 200 OK\r\nX-OAuth-Scopes: repo\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n")
            .unwrap();
        assert_eq!(a.status, 200);
        assert_eq!(a.header("x-oauth-scopes"), Some("repo"));
        assert_eq!(a.body, b"abcde");
        assert_eq!(format!("{a:?}"), "HttpAnswer(200, 5 bytes)");
    }
}
