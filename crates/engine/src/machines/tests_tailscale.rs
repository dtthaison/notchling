use super::addresses::AddressKind;
use super::registry::{Machine, MachineOs};
use super::runner::{CommandResult, ScriptedRunner};
use super::tailscale::*;
use std::sync::Arc;

pub const SWITCH_LIST: &str = "ID    Tailnet              Account\na1b2  person@example.com   person@example.com*\nc3d4  work.example         work@example.com\n";

pub const STATUS: &str = r#"{
  "Version": "1.80.0",
  "BackendState": "Running",
  "Self": {"HostName": "studio-mac", "DNSName": "studio-mac.tail0a1b2.ts.net.", "OS": "macOS", "UserID": 1001, "TailscaleIPs": ["100.64.0.10", "fd7a:115c:a1e0::10"], "Online": true},
  "MagicDNSSuffix": "tail0a1b2.ts.net",
  "CurrentTailnet": {"Name": "person@example.com", "MagicDNSSuffix": "tail0a1b2.ts.net", "MagicDNSEnabled": true},
  "Peer": {
    "nodekey:aa": {"ID": "n1", "HostName": "m1", "DNSName": "m1.tail0a1b2.ts.net.", "OS": "macOS", "UserID": 1001, "TailscaleIPs": ["100.64.0.11", "fd7a:115c:a1e0::11"], "Online": true},
    "nodekey:bb": {"ID": "n2", "HostName": "oracle-sg", "DNSName": "oracle-sg.tail0a1b2.ts.net.", "OS": "linux", "UserID": 1003, "TailscaleIPs": ["100.64.0.12"], "Online": false, "Tags": ["tag:server"]},
    "nodekey:cc": {"ID": "n3", "HostName": "Phone 12", "DNSName": "phone-12.tail0a1b2.ts.net.", "OS": "iOS", "UserID": 1002, "TailscaleIPs": ["100.64.0.13"], "Online": true},
    "nodekey:dd": {"ID": "n4", "HostName": "winbox", "DNSName": "winbox.tail0a1b2.ts.net.", "OS": "windows", "UserID": 1001, "TailscaleIPs": ["100.64.0.14", "fd7a:115c:a1e0::14"], "Online": true}
  },
  "User": {
    "1001": {"ID": 1001, "LoginName": "person@example.com", "DisplayName": "Person"},
    "1002": {"ID": 1002, "LoginName": "friend@example.com", "DisplayName": "Friend"},
    "1003": {"ID": 1003, "LoginName": "tagged-devices", "DisplayName": "Tagged"}
  }
}"#;

fn status() -> Status {
    parse_status(STATUS).unwrap()
}

#[test]
fn the_network_list_reads_the_active_row() {
    let list = parse_networks(SWITCH_LIST);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, "a1b2");
    assert_eq!(list[0].tailnet, "person@example.com");
    assert_eq!(list[0].account, "person@example.com");
    assert!(list[0].active);
    assert!(!list[1].active);
    assert!(list[1].is("work.example"));
    assert!(!list[1].is("work@example.com"));
    assert!(!list[0].is("work.example"));
    assert!(!list[1].is("person@example.com"));
    assert!(parse_networks("").is_empty());
    assert!(parse_networks("ID    Tailnet    Account\n").is_empty());
}

#[test]
fn the_status_gives_peers_with_their_owner_and_tags() {
    let s = status();
    assert_eq!(s.tailnet, "person@example.com");
    assert!(s.running);
    let names: Vec<&str> = s.peers.iter().map(Peer::short).collect();
    assert_eq!(names, ["m1", "oracle-sg", "phone-12", "winbox"]);
    let m1 = &s.peers[0];
    assert_eq!(m1.dns, "m1.tail0a1b2.ts.net");
    assert_eq!(m1.ipv4(), Some("100.64.0.11"));
    assert_eq!(m1.owner, "person@example.com");
    assert!(m1.online);
    let oracle = &s.peers[1];
    assert_eq!(oracle.tags, ["tag:server"]);
    assert_eq!(oracle.owner, "tagged-devices");
    assert!(!oracle.online);
}

#[test]
fn a_status_with_no_peers_or_nulls_still_reads() {
    let s = parse_status(r#"{"BackendState":"Running","Peer":null,"User":null,"CurrentTailnet":{"Name":"x@example.com"}}"#)
        .unwrap();
    assert!(s.peers.is_empty());
    let stopped = parse_status(r#"{"BackendState":"Stopped"}"#).unwrap();
    assert!(!stopped.running);
    assert!(parse_status("nonsense").is_err());
}

#[test]
fn the_filter_matches_text_system_and_online() {
    let s = status();
    let all = |f: &PeerFilter| -> Vec<&str> {
        s.peers
            .iter()
            .filter(|p| f.matches(p))
            .map(Peer::short)
            .collect()
    };
    assert_eq!(all(&PeerFilter::default()).len(), 4);
    let text = PeerFilter {
        text: "ORACLE".into(),
        ..PeerFilter::default()
    };
    assert_eq!(all(&text), ["oracle-sg"]);
    let by_owner = PeerFilter {
        text: "friend@".into(),
        ..PeerFilter::default()
    };
    assert_eq!(all(&by_owner), ["phone-12"]);
    let by_ip = PeerFilter {
        text: "100.64.0.14".into(),
        ..PeerFilter::default()
    };
    assert_eq!(all(&by_ip), ["winbox"]);
    let by_tag = PeerFilter {
        text: "tag:server".into(),
        ..PeerFilter::default()
    };
    assert_eq!(all(&by_tag), ["oracle-sg"]);
    let linux = PeerFilter {
        os: Some("Linux".into()),
        ..PeerFilter::default()
    };
    assert_eq!(all(&linux), ["oracle-sg"]);
    let online = PeerFilter {
        online_only: true,
        ..PeerFilter::default()
    };
    assert_eq!(all(&online), ["m1", "phone-12", "winbox"]);
    assert_eq!(systems(&s.peers), ["iOS", "linux", "macOS", "windows"]);
}

#[test]
fn a_machine_you_already_have_is_hidden_by_name_address_or_ip() {
    let s = status();
    let by_dns = Machine::new("a", "m1.tail0a1b2.ts.net", "agent");
    let by_short = Machine::new("b", "oracle-sg", "agent");
    let by_ip = Machine::new("c", "100.64.0.13", "agent");
    let mut by_address = Machine::new("d", "elsewhere.example", "agent");
    by_address.set_addresses(vec![
        super::addresses::MachineAddress::new(AddressKind::Lan, "192.168.1.9"),
        super::addresses::MachineAddress::new(AddressKind::TailscaleIp, "100.64.0.14"),
    ]);
    let unrelated = Machine::new("e", "m10.example", "agent");
    let shown = |machines: &[Machine]| -> Vec<String> {
        available(&s, machines)
            .iter()
            .map(|p| p.short().to_owned())
            .collect()
    };
    assert_eq!(shown(&[]), ["m1", "oracle-sg", "phone-12", "winbox"]);
    assert_eq!(
        shown(std::slice::from_ref(&by_dns)),
        ["oracle-sg", "phone-12", "winbox"]
    );
    assert_eq!(
        shown(std::slice::from_ref(&by_short)),
        ["m1", "phone-12", "winbox"]
    );
    assert_eq!(
        shown(std::slice::from_ref(&by_ip)),
        ["m1", "oracle-sg", "winbox"]
    );
    assert_eq!(shown(&[by_address]), ["m1", "oracle-sg", "phone-12"]);
    assert_eq!(shown(&[unrelated]).len(), 4);
    assert_eq!(shown(&[by_dns, by_short, by_ip]), ["winbox"]);
}

#[test]
fn an_import_saves_names_and_ips_on_the_active_network_with_no_node() {
    let s = status();
    let picked = vec![
        Picked {
            host: "m1.tail0a1b2.ts.net".into(),
            user: "sondoan".into(),
        },
        Picked {
            host: "oracle-sg.tail0a1b2.ts.net".into(),
            user: "agent".into(),
        },
    ];
    let made = import(&s, &picked, &[], None).unwrap();
    assert_eq!(made.len(), 2);
    let m1 = &made[0];
    assert_eq!(m1.name, "m1");
    assert_eq!(m1.user, "sondoan");
    assert_eq!(m1.host, "m1.tail0a1b2.ts.net");
    assert_eq!(m1.os, Some(MachineOs::MacOs));
    assert!(m1.node.is_none());
    assert!(m1.jobs.is_empty());
    assert!(m1.organization.is_none());
    assert_eq!(m1.addresses.len(), 2);
    assert_eq!(m1.addresses[0].kind, AddressKind::TailscaleName);
    assert_eq!(m1.addresses[0].host, "m1.tail0a1b2.ts.net");
    assert_eq!(
        m1.addresses[0].tailnet.as_deref(),
        Some("person@example.com")
    );
    assert_eq!(m1.addresses[1].kind, AddressKind::TailscaleIp);
    assert_eq!(m1.addresses[1].host, "100.64.0.11");
    assert_eq!(
        m1.addresses[1].tailnet.as_deref(),
        Some("person@example.com")
    );
    assert_eq!(made[1].os, Some(MachineOs::Linux));
    assert_eq!(made[1].user, "agent");
}

#[test]
fn an_import_picks_free_names_and_skips_what_is_already_there() {
    let s = status();
    let existing = vec![
        Machine::new("m1", "other.example", "agent"),
        Machine::new("phone-12", "phone-12.tail0a1b2.ts.net", "agent"),
    ];
    let picked = vec![
        Picked {
            host: "m1.tail0a1b2.ts.net".into(),
            user: "agent".into(),
        },
        Picked {
            host: "phone-12.tail0a1b2.ts.net".into(),
            user: "agent".into(),
        },
        Picked {
            host: "winbox.tail0a1b2.ts.net".into(),
            user: "agent".into(),
        },
    ];
    let made = import(&s, &picked, &existing, Some("work")).unwrap();
    let names: Vec<&str> = made.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["m1-2", "winbox"]);
    assert_eq!(made[0].organization.as_deref(), Some("work"));
    assert_eq!(made[1].os, None);
}

#[test]
fn an_import_refuses_a_bad_user_and_a_machine_that_left() {
    let s = status();
    let bad = vec![Picked {
        host: "m1.tail0a1b2.ts.net".into(),
        user: "Root".into(),
    }];
    assert!(import(&s, &bad, &[], None).is_err());
    let gone = vec![Picked {
        host: "gone.tail0a1b2.ts.net".into(),
        user: "agent".into(),
    }];
    assert!(import(&s, &gone, &[], None)
        .unwrap_err()
        .contains("no longer"));
}

#[test]
fn a_name_that_cannot_be_a_machine_name_is_made_into_one() {
    let s = status();
    let made = import(
        &s,
        &[Picked {
            host: "phone-12.tail0a1b2.ts.net".into(),
            user: "agent".into(),
        }],
        &[],
        None,
    )
    .unwrap();
    assert_eq!(made[0].name, "phone-12");
}

#[test]
fn the_cli_is_asked_read_only_and_switch_is_one_exact_command() {
    let runner = Arc::new(ScriptedRunner::new());
    runner.on(
        |a| {
            a.get(1).map(String::as_str) == Some("switch")
                && a.get(2).map(String::as_str) == Some("--list")
        },
        CommandResult::ok(SWITCH_LIST),
    );
    runner.on(
        |a| a.get(1).map(String::as_str) == Some("status"),
        CommandResult::ok(STATUS),
    );
    let ts = Tailscale::new(runner.clone(), "/fake/tailscale");
    assert_eq!(ts.active().unwrap().id, "a1b2");
    assert_eq!(ts.status().unwrap().peers.len(), 4);
    assert_eq!(
        runner
            .calls()
            .iter()
            .map(|c| c.argv.clone())
            .collect::<Vec<_>>(),
        vec![
            vec!["/fake/tailscale", "switch", "--list"],
            vec!["/fake/tailscale", "status", "--json"],
        ]
    );
    ts.switch("c3d4").unwrap();
    assert_eq!(
        runner.calls().last().unwrap().argv,
        ["/fake/tailscale", "switch", "c3d4"]
    );
}

#[test]
fn switch_takes_only_a_plain_network_id() {
    assert_eq!(
        switch_argv("tailscale", "9b65").unwrap(),
        ["tailscale", "switch", "9b65"]
    );
    for bad in ["", "--list", "a b", "a;b", "-x", "../x", &"a".repeat(65)] {
        assert!(switch_argv("tailscale", bad).is_none(), "{bad}");
    }
    let runner = Arc::new(ScriptedRunner::new());
    let ts = Tailscale::new(runner.clone(), "tailscale");
    assert!(ts.switch("--accept-risk=all").is_err());
    assert!(runner.calls().is_empty());
}

#[test]
fn a_failing_cli_is_reported_in_one_line() {
    let runner = Arc::new(ScriptedRunner::new());
    runner.on(
        |_| true,
        CommandResult::new(1, "", "\nfailed to connect to local tailscaled\nmore\n"),
    );
    let ts = Tailscale::new(runner, "tailscale");
    assert_eq!(
        ts.status().unwrap_err(),
        "tailscale status: failed to connect to local tailscaled"
    );
    assert!(ts.networks().is_err());
    assert!(ts.active().is_none());
}

#[test]
fn two_networks_on_one_login_are_told_apart_by_the_network() {
    let list = parse_networks(
        "ID    Tailnet              Account\na1b2  person@example.com   person@example.com*\nc3d4  work.example         person@example.com\n",
    );
    assert!(list[0].is("person@example.com"));
    assert!(!list[0].is("work.example"));
    assert!(list[1].is("work.example"));
    assert!(!list[1].is("person@example.com"));
}

#[test]
fn a_peer_with_an_unsafe_name_is_not_offered_or_imported() {
    let mut s = status();
    s.peers[0].dns = "m1,evil.tail0a1b2.ts.net".to_owned();
    s.peers[1].dns = String::new();
    s.peers[1].name = "-oProxyCommand=x".to_owned();
    s.peers[1].ips = Vec::new();
    s.peers[2].dns = String::new();
    s.peers[2].ips = vec!["not-an-ip".to_owned()];
    let names: Vec<&str> = available(&s, &[]).into_iter().map(Peer::short).collect();
    assert_eq!(names, ["winbox"]);
    let bad = Picked {
        host: s.peers[0].host().to_owned(),
        user: "me".to_owned(),
    };
    assert!(import(&s, &[bad], &[], None).is_err());
}

#[test]
fn a_network_name_with_option_characters_is_refused_on_import() {
    let mut s = status();
    s.tailnet = "-oProxyCommand=x".to_owned();
    let pick = Picked {
        host: "m1.tail0a1b2.ts.net".to_owned(),
        user: "me".to_owned(),
    };
    assert!(import(&s, &[pick], &[], None).is_err());
}
