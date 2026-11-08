//! The HTTP the providers make, behind a trait so no test reaches a network: Codex's usage
//! endpoint on chatgpt.com (the system's root certificates) and Antigravity's language server on
//! the loopback address (a self-signed certificate, trusted for `127.0.0.1` only).
//!
//! A request carries tokens in its headers, so it never prints them: [`HttpRequest`]'s `Debug`
//! names the headers and shows no values.

use std::sync::Mutex;
use std::time::Duration;

/// Whose certificate the server may present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    /// The operating system's roots.
    System,
    /// Any certificate, but only from `https://127.0.0.1:<port>`.
    LoopbackSelfSigned,
}

#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// `GET` or `POST`.
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
    pub trust: Trust,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(k, _)| k.as_str()).collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &names)
            .field("body_bytes", &self.body.as_ref().map(Vec::len))
            .field("trust", &self.trust)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Sends one request. The error text never holds a header or a body.
pub trait HttpClient: Send + Sync {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String>;
}

/// Records every request and answers from a queue (an empty queue is an error), for tests. Tests
/// of a switched-off daemon check that [`FakeHttp::calls`] stayed 0.
#[derive(Default)]
pub struct FakeHttp {
    seen: Mutex<Vec<HttpRequest>>,
    answers: Mutex<std::collections::VecDeque<Result<HttpResponse, String>>>,
}

impl FakeHttp {
    pub fn new() -> FakeHttp {
        FakeHttp::default()
    }

    /// The next call answers this.
    pub fn push(&self, answer: Result<HttpResponse, String>) {
        lock(&self.answers).push_back(answer);
    }

    pub fn ok(&self, status: u16, body: &str) {
        self.push(Ok(HttpResponse {
            status,
            body: body.as_bytes().to_vec(),
        }));
    }

    pub fn requests(&self) -> Vec<HttpRequest> {
        lock(&self.seen).clone()
    }

    pub fn calls(&self) -> usize {
        lock(&self.seen).len()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl HttpClient for FakeHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        lock(&self.seen).push(request.clone());
        lock(&self.answers)
            .pop_front()
            .unwrap_or_else(|| Err("no answer queued".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_never_prints_its_header_values() {
        let request = HttpRequest {
            method: "GET",
            url: "https://example.test/x".into(),
            headers: vec![("Authorization".into(), "Bearer secret-token".into())],
            body: Some(b"secret-body".to_vec()),
            timeout: Duration::from_secs(1),
            trust: Trust::System,
        };
        let shown = format!("{request:?}");
        assert!(shown.contains("Authorization"));
        assert!(!shown.contains("secret"), "{shown}");
    }
}
