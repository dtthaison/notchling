use crate::cluster_cli::{parse, Args, Sub};
use crate::cluster_reach::{check_addr, reach, reach_with, report, run_in, Outcome, TIMEOUT};
use crate::contract::{BAD_REQUEST, FAILED};
use std::net::{Ipv4Addr, TcpListener};
use std::time::Duration;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| w.to_string()).collect()
}

const LOOP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);

#[test]
fn parses_reach() {
    assert_eq!(
        parse(&words(&["reach", "100.101.102.103", "47823"])).unwrap(),
        Args {
            sub: Sub::Reach {
                addr: Ipv4Addr::new(100, 101, 102, 103),
                port: 47823
            },
            json: false,
            socket: None
        }
    );
    for argv in [
        &["reach"][..],
        &["reach", "100.64.0.1"],
        &["reach", "m1.ts.net", "47823"],
        &["reach", "100.64.0.1", "0"],
        &["reach", "100.64.0.1", "70000"],
        &["reach", "100.64.0.1", "x"],
        &["reach", "100.64.0.1", "47823", "extra"],
        &["reach", "100.64.0.1", "47823", "--json"],
        &["reach", "100.64.0.1", "47823", "--socket", "/tmp/s"],
    ] {
        assert!(parse(&words(argv)).is_err(), "{argv:?}");
    }
}

#[test]
fn refuses_non_tailscale_address() {
    for addr in [
        Ipv4Addr::new(10, 0, 0, 1),
        Ipv4Addr::new(192, 168, 1, 5),
        Ipv4Addr::new(100, 63, 255, 255),
        Ipv4Addr::new(100, 128, 0, 1),
        Ipv4Addr::new(8, 8, 8, 8),
        Ipv4Addr::UNSPECIFIED,
        LOOP,
    ] {
        assert!(check_addr(addr, false).is_err(), "{addr}");
    }
    assert!(check_addr(Ipv4Addr::new(100, 64, 0, 1), false).is_ok());
    assert!(check_addr(Ipv4Addr::new(100, 127, 255, 254), false).is_ok());
    assert!(check_addr(LOOP, true).is_ok());
    assert!(check_addr(Ipv4Addr::new(10, 0, 0, 1), true).is_err());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_in(
        Ipv4Addr::new(10, 0, 0, 1),
        47823,
        false,
        Duration::from_millis(50),
        &mut out,
        &mut err,
    );
    assert_eq!(code, BAD_REQUEST);
    assert!(out.is_empty());
    assert!(String::from_utf8(err).unwrap().contains("not a Tailscale"));
}

#[test]
fn reaches_a_local_listener_under_loopback() {
    let listener = TcpListener::bind((LOOP, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_in(LOOP, port, true, Duration::from_secs(2), &mut out, &mut err);
    assert_eq!(code, 0);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        format!("reached 127.0.0.1:{port}\n")
    );
    assert!(err.is_empty());
}

#[cfg(unix)]
#[test]
fn closed_port_says_refused() {
    let port = {
        let listener = TcpListener::bind((LOOP, 0)).unwrap();
        listener.local_addr().unwrap().port()
    };
    assert_eq!(reach(LOOP, port, Duration::from_secs(2)), Outcome::Refused);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_in(LOOP, port, true, Duration::from_secs(2), &mut out, &mut err);
    assert_eq!(code, FAILED);
    assert!(out.is_empty());
    assert_eq!(String::from_utf8(err).unwrap(), "refused (failed)\n");
}

#[test]
fn times_out_in_three_seconds() {
    assert_eq!(TIMEOUT, Duration::from_secs(3));
    let asked = std::cell::Cell::new(None);
    let outcome = reach_with(LOOP, 9, Duration::from_millis(20), |_, timeout| {
        asked.set(Some(timeout));
        Err(std::io::ErrorKind::TimedOut.into())
    });
    assert_eq!(outcome, Outcome::NoAnswer);
    assert_eq!(asked.get(), Some(Duration::from_millis(20)));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(report(LOOP, 9, &outcome, &mut out, &mut err), FAILED);
    assert!(out.is_empty());
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "no answer in 3 s (failed)\n"
    );
    let other = reach_with(LOOP, 9, TIMEOUT, |_, _| {
        Err(std::io::Error::other("network is unreachable"))
    });
    assert_eq!(other, Outcome::Failed("network is unreachable".to_string()));
}
