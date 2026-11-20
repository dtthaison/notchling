use super::addresses::{AddressKind, MachineAddress};
use super::forms::abilities_line;
use super::names::{connect_hosts, short_host};
use super::registry::{MachineAbilities, MachineOs};
use super::specs::{cpu_model, disk_gb, ram_gb};
use super::step::StepId;
use super::tests_machine_steps::{check, linux, parse, script, step};

const FULL: &str = "oracle-sg.tail6e77ec.ts.net";

#[test]
fn a_ts_net_host_shows_its_first_label() {
    assert_eq!(short_host(FULL), "oracle-sg");
    assert_eq!(short_host("Oracle-SG.Tail6e77ec.TS.NET"), "Oracle-SG");
    assert_eq!(short_host("box.ts.net"), "box");
}

#[test]
fn other_hosts_are_shown_whole() {
    for host in [
        "vps-sing-1",
        "100.101.102.103",
        "build.example.com",
        "fd7a:115c::1",
        "ts.net",
        ".ts.net",
        "mts.net",
        "host.ts.net.example.com",
    ] {
        assert_eq!(short_host(host), host, "{host}");
    }
}

#[test]
fn a_tailscale_name_connects_by_the_short_name_then_the_full_one() {
    let address = MachineAddress::new(AddressKind::TailscaleName, FULL);
    assert_eq!(
        connect_hosts(&address),
        vec!["oracle-sg".to_owned(), FULL.to_owned()]
    );
}

#[test]
fn only_a_tailscale_name_gets_a_short_try() {
    let ip = MachineAddress::new(AddressKind::TailscaleIp, "100.64.0.9");
    assert_eq!(connect_hosts(&ip), vec!["100.64.0.9".to_owned()]);
    let public = MachineAddress::new(AddressKind::Public, FULL);
    assert_eq!(connect_hosts(&public), vec![FULL.to_owned()]);
    let plain = MachineAddress::new(AddressKind::TailscaleName, "vps-sing-1");
    assert_eq!(connect_hosts(&plain), vec!["vps-sing-1".to_owned()]);
}

#[test]
fn ram_rounds_up_and_disk_rounds_to_the_nearest() {
    assert_eq!(ram_gb(16_777_216), Some(16));
    assert_eq!(ram_gb(11_900_000), Some(12));
    assert_eq!(ram_gb(1), Some(1));
    assert_eq!(ram_gb(0), None);
    assert_eq!(ram_gb(-5), None);
    assert_eq!(disk_gb(209_715_200), Some(200));
    assert_eq!(disk_gb(202_000_000), Some(193));
    assert_eq!(disk_gb(100), Some(1));
    assert_eq!(disk_gb(0), None);
}

#[test]
fn a_cpu_model_is_tidied_and_cut() {
    assert_eq!(
        cpu_model("  Apple   M2 \t Pro "),
        Some("Apple M2 Pro".to_owned())
    );
    assert_eq!(cpu_model("   "), None);
    assert_eq!(cpu_model(&"x".repeat(200)).map(|m| m.len()), Some(80));
}

#[test]
fn reachable_reads_a_linux_machines_specs() {
    let out = "os=Linux\narch=aarch64\nhome=/home/opc\nversion=Oracle Linux Server 9.4\ndisk_kb=160432128\ndisk_total_kb=209715200\ncpus=2\ncpu_model=Neoverse-N1\nmem_kb=11900000\ndocker=none\n";
    let r = parse(StepId::REACHABLE, out, 0, &linux());
    let a = r.detected.abilities.unwrap();
    assert_eq!(a.cpu_cores, Some(2));
    assert_eq!(a.cpu_model.as_deref(), Some("Neoverse-N1"));
    assert_eq!(a.ram_gb, Some(12));
    assert_eq!(a.disk_gb, Some(200));
    assert_eq!(a.free_disk_gb, Some(153));
    assert_eq!(a.checked_at, Some(1_800_000_000));
    assert_eq!(
        abilities_line(Some(&a), Some(MachineOs::Linux)),
        "Oracle Linux Server 9.4 · aarch64 · 2 CPU · 12 GB RAM · 200 GB disk (153 free) · no Docker"
    );
}

#[test]
fn reachable_reads_a_macs_specs() {
    let out = "os=Darwin\narch=arm64\nhome=/Users/w\nversion=26.1\ndisk_kb=125829120\ndisk_total_kb=494403584\ncpus=10\ncpu_model=Apple M2 Pro\nmem_kb=33554432\n";
    let a = check(StepId::REACHABLE, out).detected.abilities.unwrap();
    assert_eq!(a.cpu_cores, Some(10));
    assert_eq!(a.cpu_model.as_deref(), Some("Apple M2 Pro"));
    assert_eq!(a.ram_gb, Some(32));
    assert_eq!(a.disk_gb, Some(472));
}

#[test]
fn missing_or_bad_specs_are_left_out_and_old_values_stay() {
    let mut machine = linux();
    machine.abilities.as_mut().unwrap().cpu_cores = Some(8);
    let out = "os=Linux\narch=x86_64\nhome=/home/a\nversion=Debian 12\ncpus=zero\nmem_kb=\ndisk_total_kb=-4\ncpu_model=\n";
    let a = parse(StepId::REACHABLE, out, 0, &machine)
        .detected
        .abilities
        .unwrap();
    assert_eq!(a.cpu_cores, Some(8));
    assert_eq!(a.ram_gb, None);
    assert_eq!(a.disk_gb, None);
    assert_eq!(a.cpu_model, None);
    let line = abilities_line(Some(&a), Some(MachineOs::Linux));
    assert!(line.contains("8 CPU") && !line.contains("RAM") && !line.contains("disk"));
}

#[test]
fn the_probe_reads_only() {
    let s = step(StepId::REACHABLE, &linux());
    let text = script(&s);
    for word in [
        "nproc",
        "/proc/meminfo",
        "sysctl -n hw.ncpu",
        "hw.memsize",
        "machdep.cpu.brand_string",
        "df -Pk",
    ] {
        assert!(text.contains(word), "{word}");
    }
    for word in ["install", "sudo", "rm ", "write", ">>", "brew", "apt"] {
        assert!(!text.contains(word), "{word}");
    }
}

#[test]
fn the_disk_total_stands_in_for_the_free_figure_only_when_both_are_known() {
    let both = MachineAbilities {
        disk_gb: Some(200),
        free_disk_gb: Some(153),
        ..MachineAbilities::default()
    };
    assert_eq!(
        abilities_line(Some(&both), Some(MachineOs::Linux)),
        "Linux · 200 GB disk (153 free)"
    );
    let total = MachineAbilities {
        disk_gb: Some(200),
        ..MachineAbilities::default()
    };
    assert_eq!(
        abilities_line(Some(&total), Some(MachineOs::Linux)),
        "Linux · 200 GB disk"
    );
}

#[test]
fn an_old_machines_file_parses_unchanged() {
    let old = r#"{"arch":"arm64","osVersion":"26.1","freeDiskGB":120,"docker":"none"}"#;
    let a: MachineAbilities = serde_json::from_str(old).unwrap();
    assert_eq!(a.free_disk_gb, Some(120));
    assert_eq!(a.cpu_cores, None);
    assert_eq!(a.ram_gb, None);
    assert_eq!(a.checked_at, None);
    let again = serde_json::to_string(&a).unwrap();
    assert!(!again.contains("cpu") && !again.contains("ramGB") && !again.contains("checkedAt"));
}

#[test]
fn the_new_fields_round_trip_under_their_names() {
    let a = MachineAbilities {
        cpu_cores: Some(2),
        cpu_model: Some("Neoverse-N1".into()),
        ram_gb: Some(12),
        disk_gb: Some(200),
        checked_at: Some(1_800_000_000),
        ..MachineAbilities::default()
    };
    let text = serde_json::to_string(&a).unwrap();
    for key in ["cpuCores", "cpuModel", "ramGB", "diskGB", "checkedAt"] {
        assert!(text.contains(key), "{key}");
    }
    assert_eq!(serde_json::from_str::<MachineAbilities>(&text).unwrap(), a);
}
