use super::*;
use crate::bridge::net::access::tests::{configured, gate_with};
use crate::bridge::net::jwt::tests::{claims, good_claims, TestClock, TestKey, START, TEAM};
use crate::bridge::net::limiter::AuthRateLimiter;
use std::sync::Arc;

const KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";

fn head(text: &str) -> HeadParse {
    HeadReader::new(START).feed(text.as_bytes())
}

struct Req<'a> {
    extra: Vec<String>,
    path: &'a str,
    connection: &'a str,
    version: &'a str,
    key: Option<&'a str>,
    protocols: Option<&'a str>,
}

impl Default for Req<'_> {
    fn default() -> Self {
        Req {
            extra: Vec::new(),
            path: "/v1",
            connection: "Upgrade",
            version: "13",
            key: Some(KEY),
            protocols: Some("agentconsole.v1"),
        }
    }
}

impl Req<'_> {
    fn text(&self) -> String {
        let mut lines = vec![
            format!("GET {} HTTP/1.1", self.path),
            "Host: console.trietkun.shop".into(),
            format!("Connection: {}", self.connection),
            "Upgrade: websocket".into(),
            format!("Sec-Websocket-Version: {}", self.version),
        ];
        if let Some(key) = self.key {
            lines.push(format!("Sec-Websocket-Key: {key}"));
        }
        if let Some(p) = self.protocols {
            lines.push(format!("Sec-Websocket-Protocol: {p}"));
        }
        lines.extend(self.extra.iter().cloned());
        format!("{}\r\n\r\n", lines.join("\r\n"))
    }
}

fn parsed(text: &str) -> HttpHead {
    match head(text) {
        HeadParse::Complete(head, _) => head,
        other => panic!("did not parse: {text} {other:?}"),
    }
}

#[test]
fn parses_a_cloudflared_style_request_case_insensitively() {
    let req = Req {
        extra: vec!["Cf-Connecting-Ip: 203.0.113.9".into()],
        ..Req::default()
    };
    let parsed = parsed(&req.text());
    assert_eq!(parsed.method, "GET");
    assert_eq!(parsed.path(), "/v1");
    assert_eq!(parsed.value("sec-websocket-key"), Some(KEY));
    assert_eq!(parsed.value("CF-CONNECTING-IP"), Some("203.0.113.9"));
}

#[test]
fn a_head_split_across_reads_keeps_the_leftover_bytes() {
    let mut reader = HeadReader::new(START);
    let text = Req::default().text();
    assert_eq!(reader.feed(&text.as_bytes()[..20]), HeadParse::NeedMore);
    match reader.feed(format!("{}XY", &text[20..]).as_bytes()) {
        HeadParse::Complete(_, leftover) => assert_eq!(leftover, b"XY"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_connection_header_with_several_tokens_still_means_upgrade() {
    let parsed = parsed(
        &Req {
            connection: "keep-alive, Upgrade",
            ..Req::default()
        }
        .text(),
    );
    assert!(parsed.tokens("Connection").contains(&"upgrade".to_string()));
}

#[test]
fn malformed_heads_are_400() {
    for text in [
        "GARBAGE\r\n\r\n",
        "GET /v1 HTTP/1.0\r\nHost: a\r\n\r\n",
        "GET v1 HTTP/1.1\r\n\r\n",
        "GET /v1 HTTP/1.1\r\nBad Name: x\r\n\r\n",
        "GET /v1 HTTP/1.1\r\nNoColon\r\n\r\n",
        "GET /v1 HTTP/1.1\r\n folded: x\r\n\r\n",
    ] {
        assert!(matches!(head(text), HeadParse::Failure(400, _)), "{text}");
    }
    assert!(matches!(
        head("GET /v1 HTTP/1.1\r\nX: \u{0}\u{ff}\r\n\r\n"),
        HeadParse::Complete(..)
    ));
    let mut reader = HeadReader::new(START);
    let mut bytes = b"GET /v1 HTTP/1.1\r\nX: ".to_vec();
    bytes.extend_from_slice(&[0xFF, 0xFE]);
    bytes.extend_from_slice(b"\r\n\r\n");
    assert!(
        matches!(reader.feed(&bytes), HeadParse::Failure(400, _)),
        "not text"
    );
}

#[test]
fn an_oversized_head_is_refused_without_waiting_for_the_end() {
    let mut reader = HeadReader::new(START);
    let junk = format!(
        "GET /v1 HTTP/1.1\r\nX-Pad: {}",
        "a".repeat(MAX_HEAD_BYTES + 10)
    );
    assert!(matches!(
        reader.feed(junk.as_bytes()),
        HeadParse::Failure(400, _)
    ));
}

#[test]
fn a_slow_client_times_out_after_five_seconds() {
    let mut reader = HeadReader::new(START);
    assert_eq!(reader.feed(b"GET /v1 HT"), HeadParse::NeedMore);
    assert!(!reader.timed_out(START + 4900));
    assert!(reader.timed_out(START + 5000));
}

#[test]
fn the_accept_key_matches_the_rfc_6455_example() {
    assert_eq!(accept_value(KEY), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    assert!(is_valid_key(KEY));
    assert!(!is_valid_key("short"));
    assert!(!is_valid_key("dGhlIHNhbXBsZSBub25jZQ"));
}

fn gate(
    enabled: bool,
) -> (
    crate::bridge::net::access::TunnelGate,
    TestKey,
    TestClock,
    Arc<AuthRateLimiter>,
) {
    let key = TestKey::a("k1");
    let clock = TestClock::new(START);
    let limiter = Arc::new(AuthRateLimiter::new());
    let gate = gate_with(configured(enabled), &key, &clock, limiter.clone());
    (gate, key, clock, limiter)
}

fn cf(extra: &[String]) -> Vec<String> {
    let mut all = vec!["Cf-Connecting-Ip: 203.0.113.9".to_string()];
    all.extend_from_slice(extra);
    all
}

#[test]
fn requests_without_cloudflare_headers_are_refused_with_403() {
    let (gate, _, _, _) = gate(true);
    let outcome = tunnel_upgrade(&parsed(&Req::default().text()), &gate);
    assert_eq!(outcome.status(), 403);
    let Upgrade::Refuse { response, .. } = outcome else {
        panic!()
    };
    assert!(String::from_utf8(response)
        .unwrap()
        .starts_with("HTTP/1.1 403 Forbidden\r\n"));
}

#[test]
fn every_refusal_is_a_real_http_status() {
    let (gate, key, clock, _) = gate(true);
    let code = |req: Req| tunnel_upgrade(&parsed(&req.text()), &gate).status();
    assert_eq!(
        code(Req {
            extra: cf(&[]),
            path: "/other",
            ..Req::default()
        }),
        404
    );
    assert_eq!(
        code(Req {
            extra: cf(&[]),
            connection: "close",
            ..Req::default()
        }),
        426
    );
    assert_eq!(
        code(Req {
            extra: cf(&[]),
            version: "8",
            ..Req::default()
        }),
        426
    );
    assert_eq!(
        code(Req {
            extra: cf(&[]),
            key: Some("nope"),
            ..Req::default()
        }),
        400
    );
    assert_eq!(
        code(Req {
            extra: cf(&[]),
            key: None,
            ..Req::default()
        }),
        400
    );
    assert_eq!(
        code(Req {
            extra: cf(&[]),
            protocols: Some("other.v9"),
            ..Req::default()
        }),
        400
    );
    assert_eq!(
        code(Req {
            extra: cf(&[]),
            ..Req::default()
        }),
        401,
        "no Access token"
    );
    let garbage = cf(&["Cf-Access-Jwt-Assertion: garbage".into()]);
    assert_eq!(
        code(Req {
            extra: garbage,
            ..Req::default()
        }),
        401
    );
    let other_aud = key.token(&claims(
        clock.now(),
        Some(serde_json::json!(["other"])),
        None,
        600.0,
        None,
        0.0,
    ));
    let wrong = cf(&[format!("Cf-Access-Jwt-Assertion: {other_aud}")]);
    assert_eq!(
        code(Req {
            extra: wrong,
            ..Req::default()
        }),
        401
    );
    let post = parsed("POST /v1 HTTP/1.1\r\nCf-Ray: x\r\n\r\n");
    assert_eq!(tunnel_upgrade(&post, &gate).status(), 400);
}

#[test]
fn an_expired_or_forged_jwt_is_401() {
    let (gate, key, clock, _) = gate(true);
    let expired = key.token(&claims(clock.now(), None, None, -120.0, None, -600.0));
    let forged = key.token_with(
        &good_claims(clock.now()),
        &serde_json::json!({}),
        Some(&TestKey::b("k1")),
    );
    for token in [expired, forged] {
        let req = Req {
            extra: cf(&[format!("Cf-Access-Jwt-Assertion: {token}")]),
            ..Req::default()
        };
        assert_eq!(tunnel_upgrade(&parsed(&req.text()), &gate).status(), 401);
        assert_eq!(
            lan_upgrade(&parsed(&req.text()), &gate).status(),
            401,
            "the LAN port checks it too"
        );
    }
}

#[test]
fn toggle_off_is_403_and_a_locked_out_client_is_429() {
    let (off, key, clock, _) = gate(false);
    let token = key.token(&good_claims(clock.now()));
    let good = Req {
        extra: cf(&[format!("Cf-Access-Jwt-Assertion: {token}")]),
        ..Req::default()
    }
    .text();
    assert_eq!(tunnel_upgrade(&parsed(&good), &off).status(), 403);
    let (on, _, _, limiter) = gate(true);
    limiter.record_failure("203.0.113.9", START);
    assert_eq!(tunnel_upgrade(&parsed(&good), &on).status(), 429);
}

#[test]
fn a_valid_token_gets_a_101_with_the_accept_key_and_subprotocol() {
    let (gate, key, clock, _) = gate(true);
    let token = key.token(&good_claims(clock.now()));
    let req = Req {
        extra: cf(&[format!("Cf-Access-Jwt-Assertion: {token}")]),
        connection: "keep-alive, Upgrade",
        ..Req::default()
    };
    let Upgrade::Accept { response, tunnel } = tunnel_upgrade(&parsed(&req.text()), &gate) else {
        panic!()
    };
    assert_eq!(tunnel.as_deref(), Some("203.0.113.9"));
    let reply = String::from_utf8(response).unwrap();
    assert!(reply.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
    assert!(reply.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
    assert!(reply.contains("Sec-WebSocket-Protocol: agentconsole.v1\r\n"));
    assert!(reply.ends_with("\r\n\r\n"));
}

#[test]
fn the_lan_port_takes_a_local_upgrade_and_needs_the_subprotocol() {
    let (gate, _, _, _) = gate(false);
    let local = lan_upgrade(&parsed(&Req::default().text()), &gate);
    assert!(matches!(local, Upgrade::Accept { tunnel: None, .. }));
    let other = Req {
        protocols: Some("chat"),
        ..Req::default()
    };
    assert_eq!(lan_upgrade(&parsed(&other.text()), &gate).status(), 400);
    let any_path = Req {
        path: "/",
        ..Req::default()
    };
    assert_eq!(lan_upgrade(&parsed(&any_path.text()), &gate).status(), 101);
    let _ = TEAM;
}
