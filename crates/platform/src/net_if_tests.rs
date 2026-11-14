use super::*;
use std::net::IpAddr;

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

#[test]
fn is_tailscale_ranges() {
    for yes in [
        "100.64.0.1",
        "100.127.255.254",
        "fd7a:115c:a1e0::1",
        "fd7a:115c:a1e0:ab12::7",
    ] {
        assert!(is_tailscale(&ip(yes)), "{yes}");
    }
    for no in [
        "100.128.0.1",
        "100.63.255.255",
        "10.0.0.1",
        "127.0.0.1",
        "::1",
        "fe80::1",
        "fd7a:115c:a1e1::1",
    ] {
        assert!(!is_tailscale(&ip(no)), "{no}");
    }
}

#[test]
fn mapped_v4_counts_as_v4() {
    assert!(is_tailscale(&ip("::ffff:100.100.1.2")));
    assert!(!is_tailscale(&ip("::ffff:10.0.0.1")));
}

#[test]
fn tailscale_addrs_are_only_tailscale() {
    assert!(tailscale_addrs().iter().all(is_tailscale));
}
