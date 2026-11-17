use super::address::{parse, Address};

fn ok(typed: &str) -> Address {
    parse(typed, "tester", &[]).unwrap_or_else(|e| panic!("{typed}: {e}"))
}

fn refused(typed: &str) -> String {
    parse(typed, "tester", &[]).expect_err(typed)
}

#[test]
fn user_at_host() {
    assert_eq!(
        ok("agent@vps-sing-1"),
        Address {
            name: "vps-sing-1".into(),
            host: "vps-sing-1".into(),
            user: "agent".into()
        }
    );
    assert_eq!(ok("  agent@m2  ").host, "m2");
}

#[test]
fn bare_host_takes_this_user() {
    let a = ok("m2");
    assert_eq!((a.user.as_str(), a.host.as_str()), ("tester", "m2"));
    let b = parse("m2", "sondoan", &[]).unwrap();
    assert_eq!(b.user, "sondoan");
    assert!(parse("m2", "Bad User", &[]).is_err());
}

#[test]
fn tailnet_name_gives_short_name() {
    let a = ok("agent@m1.tail1234.ts.net");
    assert_eq!(a.name, "m1");
    assert_eq!(a.host, "m1.tail1234.ts.net");
    assert_eq!(ok("agent@Mac-Mini.local").name, "mac-mini");
    assert_eq!(ok("agent@BOX-2.tail1234.ts.net").name, "box-2");
}

#[test]
fn ipv4_name() {
    let a = ok("agent@100.101.102.7");
    assert_eq!(a.name, "machine-7");
    assert_eq!(a.host, "100.101.102.7");
    assert_eq!(ok("100.64.0.230").name, "machine-230");
}

#[test]
fn ipv6_in_brackets() {
    let a = ok("agent@[fd7a:115c:a1e0::1234]");
    assert_eq!(a.host, "fd7a:115c:a1e0::1234");
    assert_eq!(a.user, "agent");
    assert_eq!(a.name, "machine-1234");
    assert_eq!(ok("[fd7a:115c:a1e0::5]").user, "tester");
    assert!(!refused("agent@fd7a:115c:a1e0::1234").is_empty());
    assert!(!refused("agent@[fd7a:115c:a1e0::1234]:22").is_empty());
    assert!(!refused("agent@[not-an-address]").is_empty());
    assert!(!refused("agent@[fd7a::1").is_empty());
}

#[test]
fn root_is_refused() {
    for typed in ["root@m1", "root@100.64.0.1"] {
        let why = refused(typed);
        assert!(why.contains("own account"), "{why}");
    }
    assert!(parse("m1", "root", &[])
        .unwrap_err()
        .contains("own account"));
}

#[test]
fn port_is_refused() {
    for typed in ["agent@m1:22", "m1:2222", "agent@100.64.0.1:22"] {
        let why = refused(typed);
        assert!(why.contains("port"), "{typed}: {why}");
    }
}

#[test]
fn path_scheme_and_empty_parts_are_refused() {
    for typed in [
        "ssh://agent@m1",
        "SSH://m1",
        "agent@m1/home",
        "agent@m1/",
        "/agent@m1",
        "agent@m1:/srv",
        "",
        "   ",
        "@m1",
        "agent@",
        "@",
        "agent@m1@m2",
        "agent@m 1",
        "agent @m1",
        "agent@m1 -oProxyCommand=x",
        "-oProxyCommand=x@m1",
        "agent@-m1",
    ] {
        assert!(!refused(typed).is_empty(), "{typed:?}");
    }
}

#[test]
fn name_taken_is_refused() {
    let existing = vec!["m1".to_string()];
    let why = parse("agent@m1.tail1234.ts.net", "tester", &existing).unwrap_err();
    assert!(why.contains("already a machine"), "{why}");
    assert!(parse("agent@m2", "tester", &existing).is_ok());
}

#[test]
fn control_characters_refused() {
    for typed in [
        "agent@m1\n",
        "agent@m1\r\n",
        "agent@m1\0",
        "agent@m\t1",
        "agent@m1\u{1b}[0m",
        "agent\n@m1",
        "agent@m1\u{7f}",
        "\tagent@m1",
    ] {
        assert!(!refused(typed).is_empty(), "{typed:?}");
    }
}

#[test]
fn every_suggested_name_passes_the_name_rule() {
    for typed in [
        "agent@[::ffff:192.0.2.128]",
        "agent@[::192.0.2.7]",
        "agent@[64:ff9b::198.51.100.9]",
        "agent@[::ffff:192.0.2.0]",
        "agent@[fe80::1]",
        "agent@[2001:db8::abcd]",
        "agent@[::1]",
    ] {
        let a = ok(typed);
        assert!(
            super::forms::machine_problems(&a.name, &a.host, &a.user, &[]).is_empty(),
            "{typed}: {}",
            a.name
        );
    }
    assert_eq!(ok("agent@[::ffff:192.0.2.128]").name, "machine-128");
}
