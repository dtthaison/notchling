use crate::testutil::{closed, line, opened, quiet, Rec, Scratch};
use crate::{
    allowed, load_or_make, Identity, MemNet, MemTransport, Pins, TcpTls, Transport, MAX_LINE,
};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

fn ids(n: usize) -> (Scratch, Vec<Identity>) {
    let scratch = Scratch::new();
    let ids = (0..n)
        .map(|i| load_or_make(&scratch.path().join(format!("n{i}"))).unwrap())
        .collect();
    (scratch, ids)
}

fn pins(fps: &[&str]) -> Pins {
    let pins = Pins::default();
    pins.set(fps.iter().map(|s| s.to_string()));
    pins
}

fn loopback() -> Vec<String> {
    vec!["127.0.0.1".to_string()]
}

#[test]
fn pinned_pair_connects_on_loopback() {
    let (_s, ids) = ids(2);
    let (a, b) = (&ids[0], &ids[1]);
    let ta = TcpTls::new(a.clone(), pins(&[&b.fingerprint]), true);
    let tb = TcpTls::new(b.clone(), pins(&[&a.fingerprint]), true);
    let (rb, eb) = Rec::new();
    let listener = tb.listen(0, rb).unwrap();
    let port = listener.ports()[0];
    let (ra, ea) = Rec::new();
    let conn = ta.dial(&loopback(), port, &b.fingerprint, ra).unwrap();
    assert_eq!(conn.peer_fingerprint(), b.fingerprint);
    assert_eq!(opened(&ea).id(), conn.id());
    let served = opened(&eb);
    assert_eq!(served.peer_fingerprint(), a.fingerprint);
    conn.send(b"{\"type\":\"ping\"}\n".to_vec()).unwrap();
    assert_eq!(
        line(&eb),
        (a.fingerprint.clone(), b"{\"type\":\"ping\"}".to_vec())
    );
    served.send(b"yo\n".to_vec()).unwrap();
    assert_eq!(line(&ea), (b.fingerprint.clone(), b"yo".to_vec()));
    conn.close();
    assert_eq!(closed(&eb), a.fingerprint);
    assert!(!conn.is_open());
}

#[test]
fn unpinned_peer_fails_handshake() {
    let (_s, ids) = ids(3);
    let (a, b, c) = (&ids[0], &ids[1], &ids[2]);
    let ta = TcpTls::new(a.clone(), pins(&[&b.fingerprint]), true);
    let tb = TcpTls::new(b.clone(), pins(&[&c.fingerprint]), true);
    let (rb, eb) = Rec::new();
    let listener = tb.listen(0, rb).unwrap();
    let (ra, ea) = Rec::new();
    let dialed = ta.dial(&loopback(), listener.ports()[0], &b.fingerprint, ra);
    assert!(dialed.is_err(), "the dialer hears the refusal");
    assert!(quiet(&eb, Duration::from_millis(300)), "nothing opened");
    assert!(quiet(&ea, Duration::from_millis(50)));
}

#[test]
fn wrong_server_fingerprint_fails() {
    let (_s, ids) = ids(3);
    let (a, b, c) = (&ids[0], &ids[1], &ids[2]);
    let ta = TcpTls::new(a.clone(), pins(&[&b.fingerprint]), true);
    let tb = TcpTls::new(b.clone(), pins(&[&a.fingerprint]), true);
    let (rb, eb) = Rec::new();
    let listener = tb.listen(0, rb).unwrap();
    let (ra, _ea) = Rec::new();
    assert!(ta
        .dial(&loopback(), listener.ports()[0], &c.fingerprint, ra)
        .is_err());
    assert!(quiet(&eb, Duration::from_millis(300)));
}

#[test]
fn tls12_is_refused() {
    let (_s, ids) = ids(2);
    let (a, b) = (&ids[0], &ids[1]);
    let tb = TcpTls::new(b.clone(), pins(&[&a.fingerprint]), true);
    let (rb, eb) = Rec::new();
    let listener = tb.listen(0, rb).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS12])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(crate::tls::server_verifier(&b.fingerprint))
        .with_client_auth_cert(vec![a.cert_der.clone().into()], a.private_key())
        .unwrap();
    let name = rustls_pki_types::ServerName::try_from("notchling-node").unwrap();
    let mut client = rustls::ClientConnection::new(Arc::new(config), name).unwrap();
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", listener.ports()[0])).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut failed = false;
    for _ in 0..20 {
        if client.complete_io(&mut stream).is_err() {
            failed = true;
            break;
        }
        if !client.is_handshaking() {
            break;
        }
    }
    assert!(failed, "the listener refuses TLS 1.2");
    assert!(quiet(&eb, Duration::from_millis(300)));
}

#[test]
fn line_over_cap_closes_link() {
    let (_s, ids) = ids(2);
    let (a, b) = (&ids[0], &ids[1]);
    let ta = TcpTls::new(a.clone(), pins(&[&b.fingerprint]), true);
    let tb = TcpTls::new(b.clone(), pins(&[&a.fingerprint]), true);
    let (rb, eb) = Rec::new();
    let listener = tb.listen(0, rb).unwrap();
    let (ra, ea) = Rec::new();
    let conn = ta
        .dial(&loopback(), listener.ports()[0], &b.fingerprint, ra)
        .unwrap();
    opened(&eb);
    conn.send(vec![b'a'; MAX_LINE + 10]).unwrap();
    assert_eq!(closed(&eb), a.fingerprint);
    assert_eq!(closed(&ea), b.fingerprint);
}

#[test]
fn join_window_lets_one_unpinned_peer_in() {
    let (_s, ids) = ids(3);
    let (home, j1, j2) = (&ids[0], &ids[1], &ids[2]);
    let home_pins = Pins::default();
    let th = TcpTls::new(home.clone(), home_pins.clone(), true);
    let (rh, eh) = Rec::new();
    let listener = th.listen(0, rh).unwrap();
    let port = listener.ports()[0];
    let t1 = TcpTls::new(j1.clone(), pins(&[&home.fingerprint]), true);
    let t2 = TcpTls::new(j2.clone(), pins(&[&home.fingerprint]), true);
    let (r1, _e1) = Rec::new();
    assert!(
        t1.dial(&loopback(), port, &home.fingerprint, r1.clone())
            .is_err(),
        "no join is open"
    );
    home_pins.set_join_open(true);
    let first = t1
        .dial(&loopback(), port, &home.fingerprint, r1.clone())
        .unwrap();
    assert_eq!(opened(&eh).peer_fingerprint(), j1.fingerprint);
    let (r2, _e2) = Rec::new();
    assert!(
        t2.dial(&loopback(), port, &home.fingerprint, r2.clone())
            .is_err(),
        "one unpinned peer at a time"
    );
    first.close();
    assert_eq!(closed(&eh), j1.fingerprint);
    let second = t2.dial(&loopback(), port, &home.fingerprint, r2);
    assert!(second.is_ok(), "the slot is free again");
}

#[test]
fn tailscale_only_unless_loopback_ok() {
    let ip = |s: &str| s.parse::<IpAddr>().unwrap();
    assert!(allowed(&ip("100.100.1.2"), false));
    assert!(allowed(&ip("fd7a:115c:a1e0::5"), false));
    assert!(!allowed(&ip("127.0.0.1"), false));
    assert!(allowed(&ip("127.0.0.1"), true));
    assert!(allowed(&ip("::1"), true));
    assert!(!allowed(&ip("192.168.1.10"), true));
    assert!(!allowed(&ip("0.0.0.0"), true));
}

#[test]
fn dialing_outside_tailscale_is_refused() {
    let (_s, ids) = ids(2);
    let ta = TcpTls::new(ids[0].clone(), Pins::default(), false);
    let (ra, _ea) = Rec::new();
    let err = ta
        .dial(&loopback(), 1, &ids[1].fingerprint, ra)
        .err()
        .expect("refused");
    assert!(err.to_string().contains("Tailscale"), "{err}");
}

#[test]
fn without_loopback_the_addresses_are_tailscale_only() {
    let (_s, ids) = ids(1);
    let t = TcpTls::new(ids[0].clone(), Pins::default(), false);
    let addrs = t.addrs();
    assert!(
        addrs
            .iter()
            .all(|a| notchling_platform::net_if::is_tailscale(&a.parse().unwrap())),
        "{addrs:?}"
    );
    let dev = TcpTls::new(ids[0].clone(), Pins::default(), true);
    assert_eq!(dev.addrs(), loopback());
}

#[test]
fn mem_transport_delivers_in_order_and_checks_pins() {
    let net = MemNet::new();
    let (_s, ids) = ids(3);
    let (a, b, c) = (&ids[0], &ids[1], &ids[2]);
    let ta = MemTransport::on(&net, &a.fingerprint, pins(&[&b.fingerprint]));
    let tb = MemTransport::on(&net, &b.fingerprint, pins(&[&a.fingerprint]));
    let tc = MemTransport::on(&net, &c.fingerprint, pins(&[&b.fingerprint]));
    let (rb, eb) = Rec::new();
    let _listener = tb.listen(7, rb).unwrap();
    let (ra, ea) = Rec::new();
    let conn = ta.dial(&tb.addrs(), 7, &b.fingerprint, ra).unwrap();
    assert_eq!(opened(&ea).id(), conn.id());
    let served = opened(&eb);
    for i in 0..50 {
        conn.send(format!("{i}\n").into_bytes()).unwrap();
    }
    for i in 0..50 {
        assert_eq!(
            line(&eb),
            (a.fingerprint.clone(), format!("{i}").into_bytes())
        );
    }
    served.send(b"back\n".to_vec()).unwrap();
    assert_eq!(line(&ea).1, b"back".to_vec());
    let (rc, _ec) = Rec::new();
    assert!(
        tc.dial(&tb.addrs(), 7, &b.fingerprint, rc.clone()).is_err(),
        "c isn't pinned by b"
    );
    assert!(
        tc.dial(&tb.addrs(), 7, &a.fingerprint, rc).is_err(),
        "b isn't a"
    );
    served.close();
    assert_eq!(closed(&ea), b.fingerprint);
    assert!(conn.send(b"late\n".to_vec()).is_err());
}
