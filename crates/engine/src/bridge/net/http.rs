//! `WebSocketWire`, the HTTP half: the request head, bounded in size and time, the answers, the
//! accept key, and which upgrades each port takes.
//!
//! A client that sends too much, or too slowly, is answered with a real HTTP status and dropped;
//! nothing is left hanging.

use super::access::{is_tunnel_origin, TunnelGate};
use super::crypto::{base64, base64_decode};
use ring::digest;

pub const MAX_HEAD_BYTES: usize = 16 * 1024;
/// How long a client has to finish its TLS handshake and request head, in milliseconds.
pub const HEAD_TIMEOUT_MS: i64 = 5000;
pub const SUBPROTOCOL: &str = "agentconsole.v1";
/// The tunnel origin's one path.
pub const TUNNEL_PATH: &str = "/v1";

/// A parsed HTTP/1.1 request head.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpHead {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
}

impl HttpHead {
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Comma-separated tokens of every header called `name`, lowercased.
    pub fn tokens(&self, name: &str) -> Vec<String> {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .flat_map(|(_, v)| v.split(','))
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeadParse {
    NeedMore,
    Complete(HttpHead, Vec<u8>),
    Failure(u16, String),
}

/// Collects bytes until the blank line that ends an HTTP head.
pub struct HeadReader {
    started_ms: i64,
    buffer: Vec<u8>,
}

impl HeadReader {
    pub fn new(now_ms: i64) -> HeadReader {
        HeadReader {
            started_ms: now_ms,
            buffer: Vec::new(),
        }
    }

    pub fn timed_out(&self, now_ms: i64) -> bool {
        now_ms - self.started_ms >= HEAD_TIMEOUT_MS
    }

    pub fn feed(&mut self, data: &[u8]) -> HeadParse {
        self.buffer.extend_from_slice(data);
        let too_large = || HeadParse::Failure(400, "request head too large".into());
        let Some(end) = self.buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
            return if self.buffer.len() > MAX_HEAD_BYTES {
                too_large()
            } else {
                HeadParse::NeedMore
            };
        };
        if end + 4 > MAX_HEAD_BYTES {
            return too_large();
        }
        let leftover = self.buffer[end + 4..].to_vec();
        let Ok(text) = std::str::from_utf8(&self.buffer[..end]) else {
            return HeadParse::Failure(400, "request head is not text".into());
        };
        parse_head(text)
            .map(|head| HeadParse::Complete(head, leftover))
            .unwrap_or_else(|(status, reason)| HeadParse::Failure(status, reason.into()))
    }
}

fn parse_head(text: &str) -> Result<HttpHead, (u16, &'static str)> {
    let mut lines = text.split("\r\n");
    let request: Vec<&str> = lines.next().unwrap_or("").split(' ').collect();
    let [method, target, version] = request[..] else {
        return Err((400, "bad request line"));
    };
    if method.is_empty() || !target.starts_with('/') || version != "HTTP/1.1" {
        return Err((400, "bad request line"));
    }
    let mut headers = Vec::new();
    for line in lines {
        let Some(colon) = line.find(':').filter(|c| *c > 0) else {
            return Err((400, "bad header line"));
        };
        let name = &line[..colon];
        if name.contains([' ', '\t']) {
            return Err((400, "bad header name"));
        }
        headers.push((
            name.to_string(),
            line[colon + 1..].trim_matches([' ', '\t']).to_string(),
        ));
    }
    Ok(HttpHead {
        method: method.into(),
        target: target.into(),
        headers,
    })
}

/// An HTTP/1.1 answer that closes the connection.
pub fn response(status: u16, body: &str, headers: &[(&str, &str)]) -> Vec<u8> {
    let reason = match status {
        101 => "Switching Protocols",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        426 => "Upgrade Required",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let mut text = format!("HTTP/1.1 {status} {reason}\r\n");
    for (name, value) in headers {
        text.push_str(&format!("{name}: {value}\r\n"));
    }
    text.push_str(&format!(
        "Content-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    text.into_bytes()
}

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// The `Sec-WebSocket-Accept` for a key.
pub fn accept_value(key: &str) -> String {
    let hash = digest::digest(
        &digest::SHA1_FOR_LEGACY_USE_ONLY,
        format!("{key}{GUID}").as_bytes(),
    );
    base64(hash.as_ref())
}

/// A `Sec-WebSocket-Key` is 16 random bytes in base64.
pub fn is_valid_key(key: &str) -> bool {
    key.len() == 24
        && !key.contains(['-', '_'])
        && base64_decode(key).is_some_and(|b| b.len() == 16)
}

/// What to do with an upgrade request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Upgrade {
    /// Write `response`, then frames; `tunnel` holds the real client's address for a tunnel client.
    Accept {
        response: Vec<u8>,
        tunnel: Option<String>,
    },
    Refuse {
        response: Vec<u8>,
        status: u16,
    },
}

impl Upgrade {
    pub fn status(&self) -> u16 {
        match self {
            Upgrade::Accept { .. } => 101,
            Upgrade::Refuse { status, .. } => *status,
        }
    }
}

fn refuse(status: u16, body: &str, headers: &[(&str, &str)]) -> Upgrade {
    Upgrade::Refuse {
        response: response(status, &format!("{body}\n"), headers),
        status,
    }
}

fn accept(key: &str, tunnel: Option<String>) -> Upgrade {
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\nSec-WebSocket-Protocol: {SUBPROTOCOL}\r\n\r\n",
        accept_value(key)
    );
    Upgrade::Accept {
        response: response.into_bytes(),
        tunnel,
    }
}

/// The WebSocket checks both ports share; the key when they pass.
fn websocket_checks(head: &HttpHead) -> Result<String, Upgrade> {
    if head.method != "GET" {
        return Err(refuse(400, "GET only", &[]));
    }
    if !head.tokens("Connection").iter().any(|t| t == "upgrade")
        || !head.tokens("Upgrade").iter().any(|t| t == "websocket")
    {
        return Err(refuse(
            426,
            "websocket upgrade required",
            &[("Upgrade", "websocket")],
        ));
    }
    if head.value("Sec-WebSocket-Version") != Some("13") {
        return Err(refuse(
            426,
            "websocket version 13 required",
            &[("Sec-WebSocket-Version", "13")],
        ));
    }
    let key = head
        .value("Sec-WebSocket-Key")
        .filter(|k| is_valid_key(k))
        .ok_or_else(|| refuse(400, "bad Sec-WebSocket-Key", &[]))?;
    if !head
        .tokens("Sec-WebSocket-Protocol")
        .iter()
        .any(|t| t == SUBPROTOCOL)
    {
        return Err(refuse(400, "subprotocol agentconsole.v1 required", &[]));
    }
    Ok(key.to_string())
}

fn gate_refusal(status: u16) -> Upgrade {
    refuse(
        status,
        if status == 429 {
            "too many attempts"
        } else {
            "access refused"
        },
        &[],
    )
}

/// The tunnel origin (loopback, for cloudflared): Cloudflare's marks, `/v1`, and a valid Access
/// token. Anything without Cloudflare's headers is somebody else.
pub fn tunnel_upgrade(head: &HttpHead, gate: &TunnelGate) -> Upgrade {
    if !is_tunnel_origin(&head.headers) {
        return refuse(403, "this port is for the tunnel only", &[]);
    }
    if head.method != "GET" {
        return refuse(400, "GET only", &[]);
    }
    if head.path() != TUNNEL_PATH {
        return refuse(404, "not found", &[]);
    }
    let key = match websocket_checks(head) {
        Ok(key) => key,
        Err(refusal) => return refusal,
    };
    match gate.evaluate_tunnel(&head.headers) {
        Ok(client) => accept(&key, Some(client)),
        Err(refusal) => gate_refusal(refusal.status),
    }
}

/// The LAN port (TLS): the subprotocol is what a phone identifies itself by, as the Swift
/// listener's handshake handler did, so the path isn't checked. A request with Cloudflare's marks
/// goes through the same Access check as the tunnel.
pub fn lan_upgrade(head: &HttpHead, gate: &TunnelGate) -> Upgrade {
    let key = match websocket_checks(head) {
        Ok(key) => key,
        Err(refusal) => return refusal,
    };
    if !is_tunnel_origin(&head.headers) {
        return accept(&key, None);
    }
    match gate.evaluate_tunnel(&head.headers) {
        Ok(client) => accept(&key, Some(client)),
        Err(refusal) => gate_refusal(refusal.status),
    }
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
