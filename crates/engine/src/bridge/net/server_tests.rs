use super::*;
use crate::bridge::net::core::tests::parts;
use crate::bridge::net::core::CoreParts;
use crate::bridge::net::crypto::sha256_hex;
use crate::bridge::net::frame::masked;
use crate::bridge::net::identity::tests::make_swift_identity;
use crate::bridge::net::identity::BridgeIdentity;
use crate::bridge::net::jwt::tests::{
    claims, good_claims, jwks, FetchLog, TestClock, TestKey, AUDIENCE, TEAM,
};
use crate::bridge::test_support::FakeBackend;
use crate::sessions::testutil::TempDir;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::io::{Read, Write};

pub(crate) const SWIFT_DEVICES: &str = include_str!("testdata/devices.swift.json");
/// The token of the iPhone in the Swift-format `devices.json`.
pub(crate) const IPHONE_TOKEN: &str = "Xq3v9yR2mT7pL0wK5sJ8nB4cF6hD1gA_zE-uY2iO3pQ";
pub(crate) const PIXEL_TOKEN: &str = "Pk7nW2qR9sT4vX6yZ1aB3cD5eF8gH0jK-mN_oP2rS4t";

pub(crate) fn now_clock() -> TestClock {
    TestClock::new(crate::bridge::net::files::now_ms())
}

/// A bridge folder as the Swift app leaves it: its identity, its `devices.json` with two paired
/// phones, and internet access on with a test team.
pub(crate) fn swift_folder(dir: &TempDir) -> PathBuf {
    let bridge = dir.path("AgentConsole/bridge");
    make_swift_identity(&bridge);
    std::fs::write(bridge.join("devices.json"), SWIFT_DEVICES).unwrap();
    let access = serde_json::json!({"enabled": true, "hostname": "h.example", "teamDomain": TEAM,
        "audience": AUDIENCE, "clientId": "id", "clientSecret": "s"});
    std::fs::write(bridge.join("access.json"), access.to_string()).unwrap();
    bridge
}

use std::path::PathBuf;

struct Rig {
    _dir: TempDir,
    core: Arc<Core>,
    lan: Listener,
    tunnel: Listener,
    fingerprint: String,
    key: TestKey,
    clock: TestClock,
}

fn limits() -> Limits {
    Limits {
        head_timeout: Duration::from_millis(400),
        ..Limits::default()
    }
}

impl Rig {
    fn new() -> Rig {
        Rig::with(limits(), Arc::new(FakeBackend::default()))
    }

    fn with(limits: Limits, backend: Arc<FakeBackend>) -> Rig {
        let dir = TempDir::new("bridge-server");
        let bridge = swift_folder(&dir);
        let key = TestKey::a("k1");
        let clock = now_clock();
        let log = FetchLog::new(vec![jwks(&[&key])]);
        let core = Arc::new(Core::new(
            &bridge,
            CoreParts {
                fetch: log.fetch(),
                ..parts(backend, &dir.path("home"), clock.clock())
            },
        ));
        let identity = BridgeIdentity::load(&bridge).unwrap();
        let local = |port| SocketAddr::from(([127, 0, 0, 1], port));
        let lan = Listener::start(
            local(0),
            Kind::Lan(identity.server_config().unwrap()),
            core.clone(),
            limits,
        )
        .unwrap();
        let tunnel = Listener::start(local(0), Kind::Tunnel, core.clone(), limits).unwrap();
        Rig {
            _dir: dir,
            core,
            lan,
            tunnel,
            fingerprint: identity.fingerprint,
            key,
            clock,
        }
    }

    fn jwt(&self) -> String {
        self.key.token(&good_claims(self.clock.now()))
    }

    /// A tunnel client through the upgrade.
    fn tunnel_client(&self, ip: &str) -> TcpStream {
        let mut s = TcpStream::connect(("127.0.0.1", self.tunnel.port())).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.write_all(
            request(&[
                format!("Cf-Connecting-Ip: {ip}"),
                format!("Cf-Access-Jwt-Assertion: {}", self.jwt()),
            ])
            .as_bytes(),
        )
        .unwrap();
        let head = read_head(&mut s);
        assert!(head.starts_with("HTTP/1.1 101"), "{head}");
        s
    }

    fn hello(&self, s: &mut TcpStream, token: &str) -> serde_json::Value {
        s.write_all(&masked(
            frame::TEXT,
            format!(r#"{{"id":1,"op":"hello","token":"{token}"}}"#).as_bytes(),
            true,
        ))
        .unwrap();
        let (op, payload) = read_frame(s);
        assert_eq!(op, frame::TEXT, "{:?}", String::from_utf8_lossy(&payload));
        serde_json::from_slice(&payload).unwrap()
    }
}

fn request(extra: &[String]) -> String {
    let mut lines = vec![
        "GET /v1 HTTP/1.1".to_string(),
        "Host: console.example".into(),
        "Connection: Upgrade".into(),
        "Upgrade: websocket".into(),
        "Sec-WebSocket-Version: 13".into(),
        "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==".into(),
        "Sec-WebSocket-Protocol: agentconsole.v1".into(),
    ];
    lines.extend_from_slice(extra);
    format!("{}\r\n\r\n", lines.join("\r\n"))
}

fn read_head(s: &mut impl Read) -> String {
    let mut out = Vec::new();
    let mut byte = [0u8; 1];
    while !out.ends_with(b"\r\n\r\n") {
        match s.read(&mut byte) {
            Ok(1) => out.push(byte[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One server frame (never masked).
fn read_frame(s: &mut impl Read) -> (u8, Vec<u8>) {
    let mut head = [0u8; 2];
    s.read_exact(&mut head).unwrap();
    let mut len = (head[1] & 0x7F) as usize;
    if len == 126 {
        let mut b = [0u8; 2];
        s.read_exact(&mut b).unwrap();
        len = u16::from_be_bytes(b) as usize;
    } else if len == 127 {
        let mut b = [0u8; 8];
        s.read_exact(&mut b).unwrap();
        len = u64::from_be_bytes(b) as usize;
    }
    let mut payload = vec![0u8; len];
    s.read_exact(&mut payload).unwrap();
    (head[0] & 0x0F, payload)
}

fn read_text(s: &mut impl Read) -> String {
    loop {
        let (op, payload) = read_frame(s);
        if op == frame::TEXT {
            return String::from_utf8(payload).unwrap();
        }
    }
}

fn close_code(s: &mut impl Read) -> u16 {
    loop {
        let (op, payload) = read_frame(s);
        if op == frame::CLOSE {
            return u16::from_be_bytes([payload[0], payload[1]]);
        }
    }
}

/// True when the server has closed the socket (EOF or reset), within the read timeout.
fn closed(s: &mut TcpStream) -> bool {
    let mut buf = [0u8; 1024];
    loop {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => return true,
            Ok(_) => continue,
        }
    }
}

#[test]
fn a_tunnel_phone_says_hello_and_reaches_the_router() {
    let rig = Rig::new();
    let mut phone = rig.tunnel_client("203.0.113.9");
    let reply = rig.hello(&mut phone, IPHONE_TOKEN);
    assert_eq!(reply["ok"], true);
    assert_eq!(
        reply["data"]["device"]["id"],
        "6D0F2A58-3B77-4C0E-9A41-5E8B1C7D2F90"
    );
    assert_eq!(ping(&mut phone, 2)["ok"], true, "the router answers");
    assert_eq!(rig.core.connections(), 1);
}

/// A `ping` op; its answer.
fn ping(s: &mut TcpStream, id: u32) -> serde_json::Value {
    s.write_all(&masked(
        frame::TEXT,
        format!(r#"{{"id":{id},"op":"ping"}}"#).as_bytes(),
        true,
    ))
    .unwrap();
    let reply: serde_json::Value = serde_json::from_str(&read_text(s)).unwrap();
    assert_eq!(reply["id"], id);
    reply
}

#[test]
fn a_slow_loris_head_is_answered_408_without_stalling_other_phones() {
    let rig = Rig::new();
    let mut slow = TcpStream::connect(("127.0.0.1", rig.tunnel.port())).unwrap();
    slow.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    slow.write_all(b"GET /v1 HT").unwrap();
    let started = Instant::now();
    let mut quick = rig.tunnel_client("203.0.113.10");
    assert_eq!(
        rig.hello(&mut quick, IPHONE_TOKEN)["ok"],
        true,
        "the other phone isn't held up"
    );
    let head = read_head(&mut slow);
    assert!(head.starts_with("HTTP/1.1 408 Request Timeout"), "{head}");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(closed(&mut slow));
}

#[test]
fn a_tls_client_that_never_handshakes_is_dropped() {
    let rig = Rig::new();
    let mut silent = TcpStream::connect(("127.0.0.1", rig.lan.port())).unwrap();
    silent
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut dribble = TcpStream::connect(("127.0.0.1", rig.lan.port())).unwrap();
    dribble
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    dribble.write_all(&[0x16, 0x03]).unwrap(); // the start of a ClientHello, never finished
    let started = Instant::now();
    let mut phone = tls_phone(&rig, &rig.fingerprint).unwrap();
    assert_eq!(
        ws_hello(&mut phone, PIXEL_TOKEN)["ok"],
        true,
        "the other phone isn't held up"
    );
    assert!(closed(&mut silent));
    assert!(closed(&mut dribble));
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn a_frame_of_256_kb_plus_one_is_refused_and_others_carry_on() {
    let rig = Rig::new();
    let mut other = rig.tunnel_client("203.0.113.11");
    rig.hello(&mut other, PIXEL_TOKEN);
    let mut big = rig.tunnel_client("203.0.113.12");
    rig.hello(&mut big, IPHONE_TOKEN);
    let text = vec![b'a'; frame::MAX_MESSAGE_BYTES + 1];
    let _ = big.write_all(&masked(frame::TEXT, &text, true));
    let (op, payload) = read_frame(&mut big);
    assert_eq!(op, frame::CLOSE, "no answer first: the router never saw it");
    assert_eq!(u16::from_be_bytes([payload[0], payload[1]]), 1009);
    assert!(closed(&mut big));
    assert_eq!(
        ping(&mut other, 3)["ok"],
        true,
        "the other phone carries on"
    );
}

#[test]
fn a_broken_frame_closes_with_1002() {
    let rig = Rig::new();
    let mut phone = rig.tunnel_client("203.0.113.13");
    rig.hello(&mut phone, IPHONE_TOKEN);
    phone.write_all(&[0x81, 0x02, b'h', b'i']).unwrap(); // not masked
    assert_eq!(close_code(&mut phone), 1002);
    assert!(closed(&mut phone));
    let mut garbled = rig.tunnel_client("203.0.113.14");
    garbled.write_all(&[0xC1, 0x80, 0, 0, 0, 0]).unwrap(); // a reserved bit, before hello
    assert_eq!(close_code(&mut garbled), 1002);
}

#[test]
fn a_forged_or_expired_jwt_is_refused_at_the_door() {
    let rig = Rig::new();
    let expired = rig
        .key
        .token(&claims(rig.clock.now(), None, None, -3600.0, None, -7200.0));
    let forged = rig.key.token_with(
        &good_claims(rig.clock.now()),
        &serde_json::json!({}),
        Some(&TestKey::b("k1")),
    );
    for token in [expired, forged, "garbage".to_string()] {
        let mut s = TcpStream::connect(("127.0.0.1", rig.tunnel.port())).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.write_all(
            request(&[
                "Cf-Connecting-Ip: 198.51.100.1".into(),
                format!("Cf-Access-Jwt-Assertion: {token}"),
            ])
            .as_bytes(),
        )
        .unwrap();
        let head = read_head(&mut s);
        assert!(head.starts_with("HTTP/1.1 401"), "{head}");
        assert!(closed(&mut s));
    }
    let mut good = rig.tunnel_client("198.51.100.2");
    assert_eq!(rig.hello(&mut good, IPHONE_TOKEN)["ok"], true);
}

#[test]
fn a_plain_request_to_the_tunnel_port_is_403() {
    let rig = Rig::new();
    let mut s = TcpStream::connect(("127.0.0.1", rig.tunnel.port())).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(request(&[]).as_bytes()).unwrap();
    assert!(read_head(&mut s).starts_with("HTTP/1.1 403"));
}

/// Pins the bridge's certificate by its SHA-256, as the phone does with the QR's `fp`.
#[derive(Debug)]
pub(crate) struct Pinned(pub String);

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if sha256_hex(cert.as_ref()) == self.0 {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("fingerprint mismatch".into()))
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        let algs = rustls::crypto::ring::default_provider().signature_verification_algorithms;
        rustls::crypto::verify_tls12_signature(message, cert, dss, &algs)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        let algs = rustls::crypto::ring::default_provider().signature_verification_algorithms;
        rustls::crypto::verify_tls13_signature(message, cert, dss, &algs)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub(crate) type Phone =
    tungstenite::WebSocket<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>;

/// A phone on the LAN port: TLS pinned to `fingerprint`, then the WebSocket upgrade (tungstenite).
pub(crate) fn tls_phone_at(port: u16, fingerprint: &str) -> Result<Phone, String> {
    use tungstenite::client::IntoClientRequest;
    let conn = client_tls(fingerprint);
    let sock = TcpStream::connect(("127.0.0.1", port)).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let stream = rustls::StreamOwned::new(conn, sock);
    let mut req = format!("ws://127.0.0.1:{port}/v1")
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("Sec-WebSocket-Protocol", "agentconsole.v1".parse().unwrap());
    tungstenite::client(req, stream)
        .map(|(ws, _)| ws)
        .map_err(|e| e.to_string())
}

/// A phone's TLS session, pinned to `fingerprint`, before any byte has gone.
fn client_tls(fingerprint: &str) -> rustls::ClientConnection {
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(Pinned(fingerprint.to_string())))
    .with_no_client_auth();
    let name = ServerName::try_from("bridge.local").unwrap();
    rustls::ClientConnection::new(Arc::new(config), name).unwrap()
}

fn tls_phone(rig: &Rig, fingerprint: &str) -> Result<Phone, String> {
    tls_phone_at(rig.lan.port(), fingerprint)
}

pub(crate) fn ws_hello(phone: &mut Phone, token: &str) -> serde_json::Value {
    phone
        .send(tungstenite::Message::text(format!(
            r#"{{"id":1,"op":"hello","token":"{token}"}}"#
        )))
        .unwrap();
    loop {
        if let tungstenite::Message::Text(text) = phone.read().unwrap() {
            return serde_json::from_str(text.as_str()).unwrap();
        }
    }
}

#[test]
fn a_phone_paired_with_the_swift_app_connects_over_tls_with_the_same_fingerprint() {
    let rig = Rig::new();
    let mut phone = tls_phone(&rig, &rig.fingerprint).unwrap();
    let reply = ws_hello(&mut phone, PIXEL_TOKEN);
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["data"]["device"]["name"], "Pixel 9 / work");
    phone
        .send(tungstenite::Message::text(r#"{"id":2,"op":"ping"}"#))
        .unwrap();
    match phone.read().unwrap() {
        tungstenite::Message::Text(t) => assert!(t.as_str().contains(r#""ok":true"#), "{t}"),
        other => panic!("{other:?}"),
    }
    assert!(
        tls_phone(&rig, &"0".repeat(64)).is_err(),
        "another certificate fails the pin"
    );
    let mut bad = tls_phone(&rig, &rig.fingerprint).unwrap();
    assert_eq!(
        ws_hello(&mut bad, "not-a-token")["error"]["code"],
        "unauthorized"
    );
}

#[test]
fn stopping_closes_the_port_and_every_phone() {
    let rig = Rig::new();
    let mut phone = rig.tunnel_client("203.0.113.20");
    rig.hello(&mut phone, IPHONE_TOKEN);
    let (lan, tunnel) = (rig.lan.port(), rig.tunnel.port());
    let Rig {
        lan: l,
        tunnel: t,
        core,
        ..
    } = rig;
    l.stop();
    t.stop();
    assert!(closed(&mut phone));
    assert!(TcpStream::connect(("127.0.0.1", tunnel)).is_err());
    assert!(TcpStream::connect(("127.0.0.1", lan)).is_err());
    let deadline = Instant::now() + Duration::from_secs(3);
    while core.connections() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(core.connections(), 0, "the router heard it close");
}

/// True while the server keeps the socket open (a short read times out rather than ending).
fn still_open(s: &mut TcpStream) -> bool {
    s.set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let mut buf = [0u8; 64];
    match s.read(&mut buf) {
        Ok(0) => false,
        Ok(_) => true,
        Err(e) => matches!(
            e.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ),
    }
}

fn wait_for(what: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !what() {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

#[test]
fn a_tls_client_dripping_its_handshake_is_closed_at_the_head_deadline() {
    let rig = Rig::new();
    let mut hello = Vec::new();
    let mut tls = client_tls(&rig.fingerprint);
    while tls.wants_write() {
        tls.write_tls(&mut hello).unwrap();
    }
    let mut s = TcpStream::connect(("127.0.0.1", rig.lan.port())).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let writer = s.try_clone().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let dripping = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            for byte in hello {
                if stop.load(Ordering::SeqCst) || (&writer).write_all(&[byte]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };
    let started = Instant::now();
    assert!(closed(&mut s));
    let took = started.elapsed();
    stop.store(true, Ordering::SeqCst);
    dripping.join().unwrap();
    assert!(
        took < Duration::from_secs(2),
        "open for {took:?} while it dripped"
    );
}

#[test]
fn a_fifth_unauthenticated_socket_from_one_address_is_refused() {
    let rig = Rig::with(
        Limits {
            head_timeout: Duration::from_secs(5),
            ..limits()
        },
        Arc::new(FakeBackend::default()),
    );
    // A phone that said hello doesn't count.
    let mut phone = tls_phone(&rig, &rig.fingerprint).unwrap();
    assert_eq!(ws_hello(&mut phone, PIXEL_TOKEN)["ok"], true);
    let connect = || TcpStream::connect(("127.0.0.1", rig.lan.port())).unwrap();
    let mut idle: Vec<TcpStream> = (0..4).map(|_| connect()).collect();
    assert!(wait_for(|| rig.lan.sockets() == 5));
    let mut fifth = connect();
    fifth
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let started = Instant::now();
    assert!(closed(&mut fifth));
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the fifth stayed {:?}",
        started.elapsed()
    );
    for s in &mut idle {
        assert!(still_open(s), "the first four stay");
    }
    drop(idle.pop());
    assert!(wait_for(|| rig.lan.sockets() == 4));
    let mut again = connect();
    assert!(still_open(&mut again), "one gone makes room for another");
}

#[test]
fn an_unauthenticated_websocket_answering_pings_is_closed_at_the_auth_deadline() {
    let rig = Rig::with(
        Limits {
            auth_timeout: Duration::from_millis(600),
            ping_every: Duration::from_millis(100),
            ..limits()
        },
        Arc::new(FakeBackend::default()),
    );
    let mut phone = tls_phone(&rig, &rig.fingerprint).unwrap();
    let started = Instant::now();
    let mut pings = 0;
    while started.elapsed() < Duration::from_secs(4) {
        match phone.read() {
            Ok(tungstenite::Message::Ping(_)) => pings += 1,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let took = started.elapsed();
    assert!(pings > 0, "it answered pings");
    assert!(
        took < Duration::from_millis(1500),
        "open for {took:?} without a hello"
    );
}

#[test]
fn a_pair_waiting_for_approval_outlasts_the_auth_deadline_and_then_may_idle() {
    let backend = Arc::new(FakeBackend::default());
    backend.with(|s| s.approval_delay_ms = 1500);
    let rig = Rig::with(
        Limits {
            auth_timeout: Duration::from_millis(600),
            ..limits()
        },
        backend,
    );
    let code = rig.core.codes.issue(rig.core.now()).unwrap().code;
    let mut phone = tls_phone(&rig, &rig.fingerprint).unwrap();
    phone
        .send(tungstenite::Message::text(format!(
            r#"{{"id":1,"op":"pair","code":"{code}","device":{{"name":"Test","platform":"ios"}}}}"#
        )))
        .unwrap();
    let reply = loop {
        if let tungstenite::Message::Text(t) = phone.read().expect("open while the pair waits") {
            break serde_json::from_str::<serde_json::Value>(t.as_str()).unwrap();
        }
    };
    assert_eq!(reply["ok"], true, "{reply}");
    std::thread::sleep(Duration::from_secs(1));
    phone
        .send(tungstenite::Message::text(r#"{"id":2,"op":"ping"}"#))
        .unwrap();
    match phone.read().expect("an idle paired phone stays") {
        tungstenite::Message::Text(t) => assert!(t.as_str().contains(r#""ok":true"#), "{t}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_frame_larger_than_the_tls_buffer_reaches_a_tls_phone() {
    // rustls takes at most 64 KB of plaintext before its records are written out.
    let rig = Rig::new();
    let mut phone = tls_phone(&rig, &rig.fingerprint).unwrap();
    assert_eq!(ws_hello(&mut phone, PIXEL_TOKEN)["ok"], true);
    let connection = rig.core.hub.all().pop().unwrap();
    let big = "x".repeat(200 * 1024);
    connection.send_text(big.clone());
    match phone.read().expect("the frame arrives") {
        tungstenite::Message::Text(t) => assert_eq!(t.as_str().len(), big.len()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_phone_that_reads_slowly_holds_up_no_one_and_bridge_off_is_prompt() {
    let rig = Rig::with(
        Limits {
            queue: 4,
            write_timeout: Duration::from_secs(4),
            ..limits()
        },
        Arc::new(FakeBackend::default()),
    );
    let mut other = rig.tunnel_client("203.0.113.30");
    rig.hello(&mut other, IPHONE_TOKEN);
    let mut slow = tls_phone(&rig, &rig.fingerprint).unwrap();
    assert_eq!(ws_hello(&mut slow, PIXEL_TOKEN)["ok"], true);
    let connection = rig
        .core
        .hub
        .all()
        .into_iter()
        .find(|c| c.device().is_some_and(|d| d.name == "Pixel 9 / work"))
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let dripping = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut byte = [0u8; 1];
            while !stop.load(Ordering::SeqCst) {
                match slow.get_mut().sock.read(&mut byte) {
                    Ok(1) => std::thread::sleep(Duration::from_millis(50)),
                    _ => break,
                }
            }
        })
    };
    let big = "x".repeat(200 * 1024);
    let mut slowest = Duration::ZERO;
    // Spaced, so the writer is stuck in a write by the time the queue fills.
    for _ in 0..120 {
        let started = Instant::now();
        connection.send_text(big.clone());
        slowest = slowest.max(started.elapsed());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        slowest < Duration::from_millis(500),
        "a push waited {slowest:?} on the slow phone"
    );
    assert_eq!(
        ping(&mut other, 7)["ok"],
        true,
        "the other phone carries on"
    );
    let Rig { lan, tunnel, .. } = rig;
    let started = Instant::now();
    lan.stop();
    tunnel.stop();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "bridge-off took {:?}",
        started.elapsed()
    );
    stop.store(true, Ordering::SeqCst);
    dripping.join().unwrap();
}
