use notchling_engine::machines::addresses::{AddressKind, MachineAddress};

use super::add_sheet::{derived_name, parse_draft, Draft};
use super::addresses_form::{
    address_of, addresses_of, cycle, drafts_of, split_row, swap_rows, text_of, RowDraft, RowMeta,
};

fn row(text: &str) -> RowDraft {
    RowDraft {
        text: text.into(),
        meta: RowMeta::default(),
    }
}

#[test]
fn a_row_splits_into_user_host_and_port() {
    let plain = split_row("app-1.example").unwrap();
    assert_eq!(
        (plain.user, plain.host, plain.port),
        (None, "app-1.example".into(), None)
    );
    let full = split_row("deploy@10.0.0.5:2222").unwrap();
    assert_eq!(full.user.as_deref(), Some("deploy"));
    assert_eq!(full.host, "10.0.0.5");
    assert_eq!(full.port, Some(2222));
    let v6 = split_row("[fd7a::1]:22").unwrap();
    assert_eq!((v6.host.as_str(), v6.port), ("fd7a::1", Some(22)));
    let bare_v6 = split_row("fd7a::1").unwrap();
    assert_eq!((bare_v6.host.as_str(), bare_v6.port), ("fd7a::1", None));
}

#[test]
fn a_bad_row_is_refused_with_a_reason() {
    for bad in [
        "",
        "  ",
        "a b",
        "ssh://x",
        "x/y",
        "@host",
        "user@",
        "a@b@c",
        "host:0",
        "host:99999",
        "host:ab",
        "[fd7a::1",
    ] {
        assert!(split_row(bad).is_err(), "{bad:?}");
    }
    assert!(address_of(&row("bad_host!"), "deploy").is_err());
    assert!(address_of(&row("Bad User@host"), "deploy").is_err());
}

#[test]
fn a_row_becomes_an_address_with_its_kind_inferred() {
    let tailscale = address_of(&row("m1.tail0a1b2.ts.net"), "deploy").unwrap();
    assert_eq!(tailscale.kind, AddressKind::TailscaleName);
    let ip = address_of(&row("100.64.0.11"), "deploy").unwrap();
    assert_eq!(ip.kind, AddressKind::TailscaleIp);
    let lan = address_of(&row("192.168.1.20"), "deploy").unwrap();
    assert_eq!(lan.kind, AddressKind::Lan);
    let public = address_of(&row("deploy@app.example.org:2200"), "deploy").unwrap();
    assert_eq!(public.kind, AddressKind::Public);
    assert_eq!(public.user, None, "the machine's own user is not repeated");
    assert_eq!(public.port, Some(2200));
    let other = address_of(&row("root@app.example.org"), "deploy").unwrap();
    assert_eq!(other.user.as_deref(), Some("root"));
}

#[test]
fn a_chosen_kind_and_network_are_kept() {
    let mut r = row("m1.tail0a1b2.ts.net");
    r.meta.kind = Some(AddressKind::TailscaleName);
    r.meta.tailnet = Some("work.example".into());
    let a = address_of(&r, "deploy").unwrap();
    assert_eq!(a.tailnet.as_deref(), Some("work.example"));
}

#[test]
fn a_list_is_checked_as_a_whole() {
    let mut jump = row("10.0.0.9");
    jump.meta.kind = Some(AddressKind::Jump);
    let why = addresses_of(&[row("a.example"), jump], "m1", "deploy").unwrap_err();
    assert!(why.starts_with("Address 2: ") || !why.is_empty());
    let ok = addresses_of(&[row("a.example"), row("192.168.1.2")], "m1", "deploy").unwrap();
    assert_eq!(ok.len(), 2);
    assert_eq!(ok[0].host, "a.example");
    let single = addresses_of(&[row("a b")], "m1", "deploy").unwrap_err();
    assert!(!single.starts_with("Address 1"), "one row needs no number");
}

#[test]
fn rows_round_trip_through_the_text_the_user_sees() {
    let list = vec![
        MachineAddress::new(AddressKind::Lan, "192.168.1.20"),
        MachineAddress {
            port: Some(2222),
            user: Some("root".into()),
            ..MachineAddress::new(AddressKind::Public, "app.example.org")
        },
        MachineAddress {
            port: Some(22),
            ..MachineAddress::new(AddressKind::Lan, "fd7a::1")
        },
    ];
    let drafts = drafts_of(&list, "deploy");
    assert_eq!(drafts[0].text, "192.168.1.20");
    assert_eq!(drafts[1].text, "root@app.example.org:2222");
    assert_eq!(drafts[2].text, "[fd7a::1]:22");
    assert_eq!(text_of(&list[1], "root"), "app.example.org:2222");
    let back = addresses_of(&drafts, "m1", "deploy").unwrap();
    assert_eq!(back, list);
}

#[test]
fn rows_swap_and_options_cycle() {
    let mut rows = vec![row("a"), row("b"), row("c")];
    assert!(swap_rows(&mut rows, 0, 1));
    assert_eq!(rows[0].text, "b");
    assert!(!swap_rows(&mut rows, 0, 3));
    assert!(!swap_rows(&mut rows, 1, 1));
    let options = vec![None, Some("a".to_owned()), Some("b".to_owned())];
    assert_eq!(cycle(&options, &None), Some("a".to_owned()));
    assert_eq!(cycle(&options, &Some("b".to_owned())), None);
    assert_eq!(cycle(&options, &Some("gone".to_owned())), None);
    assert_eq!(cycle(&[], &None), None);
}

#[test]
fn a_plain_draft_saves_no_addresses_and_a_port_or_second_row_does() {
    let plain = Draft {
        address: "deploy@app-1.example".into(),
        ..Draft::default()
    };
    let parsed = parse_draft(&plain, "me", &[]).unwrap();
    assert!(parsed.addresses.is_empty());
    assert_eq!(parsed.target, "deploy@app-1.example");
    assert_eq!(parsed.name, "app-1");

    let ported = Draft {
        address: "deploy@app-1.example:2222".into(),
        ..Draft::default()
    };
    let parsed = parse_draft(&ported, "me", &[]).unwrap();
    assert_eq!(parsed.target, "deploy@app-1.example");
    assert_eq!(parsed.addresses.len(), 1);
    assert_eq!(parsed.addresses[0].port, Some(2222));
    assert_eq!(derived_name("deploy@app-1.example:2222", "me"), "app-1");

    let more = Draft {
        address: "deploy@app-1.example".into(),
        more: vec![row("192.168.1.20"), row("  ")],
        ..Draft::default()
    };
    let parsed = parse_draft(&more, "me", &[]).unwrap();
    assert_eq!(parsed.addresses.len(), 2);
    assert_eq!(parsed.addresses[0].host, "app-1.example");
    assert_eq!(parsed.addresses[1].host, "192.168.1.20");

    let bad = Draft {
        address: "deploy@app-1.example".into(),
        more: vec![row("bad host")],
        ..Draft::default()
    };
    assert!(parse_draft(&bad, "me", &[]).is_err());
}
