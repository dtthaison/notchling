use super::forms::abilities_line;
use super::names::short_host;
use super::registry::{MachineAbilities, MachineOs};
use super::specs::{cpu_model, disk_gb, free_gb, ram_gb};
use super::step::StepId;
use super::tests_machine_steps::{check, linux, parse, script, step};
use super::tests_reach::{login, ts, with, Rig, PERSON};

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

#[test]
fn a_dash_or_blank_cpu_model_is_left_out() {
    for text in ["-", " - ", "", "   ", "--"] {
        assert_eq!(cpu_model(text), None, "{text:?}");
    }
    assert_eq!(cpu_model("Cortex-A72"), Some("Cortex-A72".to_owned()));
}

#[test]
fn huge_sizes_do_not_overflow() {
    assert!(ram_gb(i64::MAX).is_some());
    assert!(disk_gb(i64::MAX).is_some());
    assert!(free_gb(i64::MAX).is_some());
    assert_eq!(free_gb(-1), None);
}

#[test]
fn free_and_total_round_the_same_way() {
    assert_eq!(free_gb(850_000), Some(1));
    assert_eq!(disk_gb(900_000), Some(1));
    assert_eq!(free_gb(0), Some(0));
    assert_eq!(free_gb(2_097_152), Some(2));
}

#[test]
fn a_stale_free_figure_never_shows_more_than_the_total() {
    let a = MachineAbilities {
        disk_gb: Some(100),
        free_disk_gb: Some(150),
        ..MachineAbilities::default()
    };
    assert_eq!(
        abilities_line(Some(&a), Some(MachineOs::Linux)),
        "Linux · 100 GB disk (100 free)"
    );
}

#[test]
fn the_probe_runs_df_once_and_reads_in_the_c_locale() {
    let text = script(&step(StepId::REACHABLE, &linux()));
    assert_eq!(text.matches("df -Pk").count(), 1);
    assert!(text.contains("LC_ALL=C lscpu"));
    assert!(text.contains("LC_ALL=C sed"));
    for word in [
        "getconf _NPROCESSORS_ONLN",
        "^processor",
        "Hardware",
        "hw.model",
    ] {
        assert!(text.contains(word), "{word}");
    }
}

#[test]
fn the_probe_output_of_an_arm_machine_with_no_model_leaves_the_model_out() {
    let out = "os=Linux\narch=aarch64\nhome=/home/opc\nversion=Oracle Linux 9\ndisk_total_kb=209715200\ndisk_kb=160432128\ncpus=4\ncpu_model=-\nmem_kb=\n";
    let a = parse(StepId::REACHABLE, out, 0, &linux())
        .detected
        .abilities
        .unwrap();
    assert_eq!(a.cpu_model, None);
    assert_eq!(a.cpu_cores, Some(4));
    assert_eq!(a.ram_gb, None);
}

#[test]
fn a_ts_net_address_is_connected_by_its_full_stored_name_without_a_try() {
    let rig = Rig::new(&[], &[]);
    let m = with("oracle", vec![ts(FULL, PERSON)]);
    let pick = rig.router.pick(&m, std::slice::from_ref(&m));
    assert_eq!(pick.choice().unwrap().route.host, FULL);
    assert!(rig.probes().is_empty());
}

#[test]
fn a_ts_net_address_is_only_ever_probed_by_its_full_name() {
    let rig = Rig::new(&[FULL], &[]);
    let ip = super::addresses::MachineAddress::new(
        super::addresses::AddressKind::TailscaleIp,
        "100.64.0.9",
    )
    .on_tailnet(PERSON);
    let m = with("oracle", vec![ts(FULL, PERSON), ip]);
    let pick = rig.router.pick(&m, std::slice::from_ref(&m));
    assert_eq!(pick.choice().unwrap().route.host, "100.64.0.9");
    let probes = rig.probes();
    assert_eq!(probes.len(), 2);
    assert_eq!(login(&probes[0]), format!("agent@{FULL}"));
    assert!(probes.iter().all(|p| !login(p).ends_with("@oracle-sg")));
}
