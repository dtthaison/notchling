use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    System,
    LoopbackSelfSigned,
}

#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
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

pub trait HttpClient: Send + Sync {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String>;
}

#[derive(Default)]
pub struct FakeHttp {
    seen: Mutex<Vec<HttpRequest>>,
    answers: Mutex<std::collections::VecDeque<Result<HttpResponse, String>>>,
}

impl FakeHttp {
    pub fn new() -> FakeHttp {
        FakeHttp::default()
    }

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
