use super::addresses::*;
use super::fakes::TempDir;
use super::registry::*;
use super::shell::{git_ssh_command, git_url, ssh_admin, ssh_script};
use crate::store;
use std::path::Path;

fn machine() -> Machine {
    let mut m = Machine::new("m1", "m1.tail0a1b2.ts.net", "agent");
    m.set_addresses(vec![
        MachineAddress::new(AddressKind::TailscaleName, "m1.tail0a1b2.ts.net")
            .on_tailnet("person@example.com"),
        MachineAddress::new(AddressKind::Lan, "192.168.1.20").with_port(2222),
    ]);
    m
}

fn doc(reg: &RegistryFiles) -> String {
    store::doc(&reg.data_dir, REGISTRY_DOC).unwrap().unwrap()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_kinds_keep_their_wire_names() {
    let text = serde_json::to_string(&AddressKind::ALL).unwrap();
    assert_eq!(
        text,
        r#"["tailscale_name","tailscale_ip","lan","public","jump"]"#
    );
    for kind in AddressKind::ALL {
        let back: AddressKind = serde_json::from_str(&format!("\"{}\"", kind.key())).unwrap();
        assert_eq!(back, kind);
    }
}

#[test]
fn a_legacy_host_gets_a_kind_from_its_shape() {
    use AddressKind::*;
    let cases = [
        ("m1", TailscaleName),
        ("m1.tail0a1b2.ts.net", TailscaleName),
        ("100.64.0.11", TailscaleIp),
        ("100.127.255.1", TailscaleIp),
        ("100.128.0.1", Public),
        ("192.168.1.9", Lan),
        ("10.1.2.3", Lan),
        ("172.16.0.1", Lan),
        ("172.32.0.1", Public),
        ("printer.local", Lan),
        ("203.0.113.5", Public),
        ("build.example.com", Public),
    ];
    for (host, kind) in cases {
        assert_eq!(infer_kind(host), kind, "{host}");
    }
}

#[test]
fn a_machine_without_addresses_has_one_from_its_host() {
    let m = Machine::new("m1", "m1.tail0a1b2.ts.net", "agent");
    assert!(!m.has_addresses());
    let list = m.address_list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].kind, AddressKind::TailscaleName);
    assert_eq!(list[0].host, "m1.tail0a1b2.ts.net");
    assert_eq!(m.route(), Route::direct("m1.tail0a1b2.ts.net", "agent"));
}

#[test]
fn the_first_address_is_the_hosts_for_a_tool_that_reads_the_file() {
    let mut m = Machine::new("m1", "old.example", "agent");
    m.set_addresses(vec![MachineAddress::new(AddressKind::Lan, "192.168.1.20")]);
    assert_eq!(m.host, "192.168.1.20");
    assert_eq!(m.user, "agent");
}

#[test]
fn an_address_is_checked() {
    let ok = MachineAddress::new(AddressKind::Lan, "192.168.1.20")
        .with_user("agent")
        .with_port(22);
    assert!(address_problems(&ok, "m1").is_empty());
    let bad = [
        (MachineAddress::new(AddressKind::Lan, "-oProxyCommand=x"), HOST),
        (MachineAddress::new(AddressKind::Lan, "a b"), HOST),
        (MachineAddress::new(AddressKind::Lan, "h").with_user("Root"), USER),
        (MachineAddress::new(AddressKind::Lan, "h").with_port(0), PORT),
        (
            MachineAddress::new(AddressKind::Lan, "h").on_tailnet("x"),
            TAILNET_KIND,
        ),
        (
            MachineAddress::new(AddressKind::TailscaleName, "h").on_tailnet("a b"),
            TAILNET,
        ),
        (MachineAddress::new(AddressKind::Jump, "h"), VIA_MISSING),
        (
            MachineAddress::new(AddressKind::Jump, "h").through("m1"),
            VIA_SELF,
        ),
        (
            MachineAddress::new(AddressKind::Lan, "h").through("m2"),
            VIA_KIND,
        ),
    ];
    for (address, why) in bad {
        assert!(address_problems(&address, "m1").contains(&why.to_owned()), "{address:?}");
    }
    assert_eq!(list_problems(&[], "m1"), vec![NONE.to_owned()]);
}

#[test]
fn normalizing_drops_what_a_kind_cannot_hold() {
    let a = MachineAddress::new(AddressKind::Lan, " 10.0.0.2 ")
        .on_tailnet("person@example.com")
        .through("m2")
        .normalized();
    assert_eq!(a.host, "10.0.0.2");
    assert_eq!(a.tailnet, None);
    assert_eq!(a.via, None);
}

#[test]
fn a_port_and_a_jump_become_ssh_options() {
    let inner = Route {
        host: "gw.example".into(),
        user: "hop".into(),
        port: Some(2200),
        jump: None,
    };
    let route = Route {
        host: "10.0.0.5".into(),
        user: "agent".into(),
        port: Some(2222),
        jump: Some(Box::new(inner.clone())),
    };
    assert_eq!(
        route.ssh_options(),
        ["-p", "2222", "-J", "hop@gw.example:2200"]
    );
    assert_eq!(
        route.scp_options(),
        ["-P", "2222", "-J", "hop@gw.example:2200"]
    );
    let outer = Route::direct("edge.example", "edge");
    let chained = Route {
        jump: Some(Box::new(Route {
            jump: Some(Box::new(outer)),
            ..inner
        })),
        ..route
    };
    assert_eq!(
        chained.jump_chain().unwrap(),
        "edge@edge.example,hop@gw.example:2200"
    );
    let v6 = Route {
        host: "fd00::1".into(),
        user: "u".into(),
        port: Some(22),
        jump: None,
    };
    let behind = Route {
        jump: Some(Box::new(v6)),
        ..Route::direct("h", "agent")
    };
    assert_eq!(behind.jump_chain().unwrap(), "u@[fd00::1]:22");
    assert!(Route::direct("h", "agent").ssh_options().is_empty());
}

#[test]
fn ssh_keeps_its_options_and_the_double_dash_with_a_route() {
    let mut m = machine();
    m.route = Some(Route {
        host: "192.168.1.20".into(),
        user: "agent".into(),
        port: Some(2222),
        jump: Some(Box::new(Route::direct("gw.example", "hop"))),
    });
    let argv = ssh_script(&m, "echo hi");
    assert_eq!(
        &argv[..11],
        [
            "/usr/bin/ssh",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=8",
            "-p",
            "2222",
            "-J",
            "hop@gw.example",
            "--",
            "agent@192.168.1.20",
        ]
    );
    let admin = ssh_admin(&m, "id", true);
    assert_eq!(admin[9], "--");
    assert_eq!(admin[10], "root@192.168.1.20");
}

#[test]
fn a_machine_without_a_route_is_reached_as_before() {
    let m = Machine::new("m1", "m1.tail", "agent");
    assert_eq!(
        ssh_script(&m, "x")[..7],
        [
            "/usr/bin/ssh",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=8",
            "--",
            "agent@m1.tail"
        ]
    );
}

#[test]
fn git_goes_to_the_picked_address() {
    let mut m = machine();
    m.route = Some(Route {
        host: "192.168.1.20".into(),
        user: "agent".into(),
        port: Some(2222),
        jump: Some(Box::new(Route::direct("gw.example", "hop"))),
    });
    assert_eq!(
        git_url(&m, "/work/tree").unwrap(),
        "ssh://agent@192.168.1.20:2222/work/tree"
    );
    assert_eq!(
        git_ssh_command(&m),
        "ssh -o BatchMode=yes -o ConnectTimeout=8 -J 'hop@gw.example'"
    );
    let plain = Machine::new("m2", "m2.tail", "agent");
    assert_eq!(
        git_ssh_command(&plain),
        "ssh -o BatchMode=yes -o ConnectTimeout=8"
    );
    assert_eq!(git_url(&plain, "relative"), None);
}

#[test]
fn addresses_survive_a_save_and_a_read() {
    let dir = TempDir::new("addr-round");
    let reg = RegistryFiles::single(&dir.join("a"));
    let mut list = vec![machine(), Machine::new("m2", "m2.tail", "agent")];
    list[0].set_addresses(vec![
        MachineAddress::new(AddressKind::TailscaleName, "m1.tail0a1b2.ts.net")
            .on_tailnet("person@example.com"),
        MachineAddress::new(AddressKind::TailscaleIp, "100.64.0.11").on_tailnet("person@example.com"),
        MachineAddress::new(AddressKind::Lan, "192.168.1.20")
            .with_user("pi")
            .with_port(2222),
        MachineAddress::new(AddressKind::Jump, "10.0.0.7").through("m2"),
    ]);
    assert!(reg.save(&list));
    let text = doc(&reg);
    assert!(text.contains("\"version\": 4"), "{text}");
    let back = reg.all();
    assert_eq!(back, list);
    assert!(back[1].addresses.is_empty());
}

#[test]
fn a_registry_with_no_addresses_stays_version_3_and_names_none() {
    let dir = TempDir::new("addr-v3");
    let reg = RegistryFiles::single(&dir.join("a"));
    assert!(reg.save(&[Machine::new("m2", "m2.tail", "agent")]));
    let text = doc(&reg);
    assert!(text.contains("\"version\": 3"), "{text}");
    assert!(!text.contains("addresses"), "{text}");
}

#[test]
fn the_first_address_is_written_as_host_and_user_beside_the_list() {
    let dir = TempDir::new("addr-mirror");
    let reg = RegistryFiles::single(&dir.join("a"));
    let mut m = Machine::new("m1", "stale.example", "agent");
    m.set_addresses(vec![
        MachineAddress::new(AddressKind::Lan, "192.168.1.20"),
        MachineAddress::new(AddressKind::Public, "203.0.113.5"),
    ]);
    assert!(reg.save(&[m]));
    let value: serde_json::Value = serde_json::from_str(&doc(&reg)).unwrap();
    let first = &value["machines"][0];
    assert_eq!(first["host"], "192.168.1.20");
    assert_eq!(first["user"], "agent");
    assert_eq!(first["addresses"][0]["host"], "192.168.1.20");
}

#[test]
fn the_first_address_save_keeps_the_version_3_file_aside_byte_for_byte() {
    let dir = TempDir::new("addr-backup");
    let reg = RegistryFiles::single(&dir.join("a"));
    let v3 = r#"{"version":3,"machines":[{"name":"m1","host":"m1.tail","user":"agent","organization":"personal"}]}"#;
    write(&reg.old_own, v3);
    let mut list = reg.all();
    assert_eq!(list.len(), 1);
    list[0].set_addresses(vec![
        MachineAddress::new(AddressKind::TailscaleName, "m1.tail"),
        MachineAddress::new(AddressKind::Lan, "192.168.1.20"),
    ]);
    assert!(reg.save(&list));
    let backup = backup_path_v3(&reg.old_own);
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), v3);
    assert!(doc(&reg).contains("\"version\": 4"));
    let kept = std::fs::read_to_string(&backup).unwrap();
    assert!(reg.save(&list));
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), kept);
}

#[test]
fn saving_a_version_3_registry_without_addresses_makes_no_backup() {
    let dir = TempDir::new("addr-nobackup");
    let reg = RegistryFiles::single(&dir.join("a"));
    write(
        &reg.old_own,
        r#"{"version":3,"machines":[{"name":"m1","host":"m1.tail","user":"agent","organization":"personal"}]}"#,
    );
    let list = reg.all();
    assert!(reg.save(&list));
    assert!(!backup_path_v3(&reg.old_own).exists());
    assert!(doc(&reg).contains("\"version\": 3"));
}

#[test]
fn an_older_build_refuses_the_file_that_holds_addresses() {
    let dir = TempDir::new("addr-old");
    let reg = RegistryFiles::single(&dir.join("a"));
    assert!(reg.save(&[machine()]));
    let value: serde_json::Value = serde_json::from_str(&doc(&reg)).unwrap();
    let version = value["version"].as_i64().unwrap();
    assert!(version > 3, "a build that knows versions up to 3 reads this as unsupported");
    assert!(value["machines"][0]["addresses"].is_array());
}

#[test]
fn a_later_version_is_never_written_over() {
    let dir = TempDir::new("addr-later");
    let reg = RegistryFiles::single(&dir.join("a"));
    write(&reg.old_own, r#"{"version":5,"machines":[]}"#);
    assert!(matches!(reg.load(), RegistryState::Unreadable(_)));
    assert!(!reg.save(&[machine()]));
    assert_eq!(
        std::fs::read_to_string(&reg.old_own).unwrap(),
        r#"{"version":5,"machines":[]}"#
    );
}

#[test]
fn a_version_4_file_loads_and_keeps_its_organization() {
    let dir = TempDir::new("addr-v4org");
    let reg = RegistryFiles::single(&dir.join("a"));
    write(
        &reg.old_own,
        r#"{"version":4,"machines":[{"name":"m1","host":"192.168.1.20","user":"agent","addresses":[{"kind":"lan","host":"192.168.1.20"}]}]}"#,
    );
    assert_eq!(reg.adopt_organizations().unwrap(), 1);
    let m = reg.machine("m1").unwrap();
    assert_eq!(m.organization.as_deref(), Some("personal"));
    assert_eq!(m.addresses.len(), 1);
    assert!(doc(&reg).contains("\"version\": 4"));
}
