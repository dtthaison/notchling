//! `machines.json` v2, the one-time v1 migration and what is never overwritten (Swift
//! `MachinesRegistryTests`). Files live in a private temp folder; nothing real is read.

use super::fakes::TempDir;
use super::registry::*;
use std::path::Path;

fn repo() -> MachineRepo {
    MachineRepo {
        github: "dtthaison/lingeva-mono".into(),
        local_path: "/Users/sondoan/go/src/lingeva/lingeva-mono".into(),
        check_command: "just check-affected".into(),
        machine_path: None,
        verify_skill: None,
    }
}

/// A machine with an sd worker job, as the Swift convenience initializer made it.
fn machine(name: &str, user: &str, python: &str, sd_path: &str, file_vault: bool) -> Machine {
    let mut m = Machine::new(name, name, user);
    m.file_vault = file_vault;
    let mut job = SdWorkerSettings::new(name);
    job.python = python.into();
    job.sd_path = sd_path.into();
    job.repos = vec![repo()];
    m.set_sd_worker(Some(job));
    m
}

fn sample() -> Vec<Machine> {
    vec![
        machine("m1", "sondoan", "python3", "/opt/sd/sd.py", true),
        machine(
            "vps-sing-1",
            "agent",
            "python3.12",
            "/home/agent/sd/sd.py",
            false,
        ),
    ]
}

fn files(dir: &TempDir, name: &str) -> RegistryFiles {
    RegistryFiles::single(&dir.join(name))
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

fn names(dir: &TempDir) -> Vec<String> {
    let mut list: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    list.sort();
    list
}

#[test]
fn registry_round_trip() {
    let dir = TempDir::new("reg");
    let reg = files(&dir, "sub/machines.json");
    assert!(reg.save(&sample()));
    assert_eq!(reg.all(), sample());
    assert_eq!(reg.machine("vps-sing-1").unwrap().user, "agent");
    assert_eq!(reg.machine("m1").unwrap().python(), "python3");
    assert_eq!(reg.machine("nope"), None);
}

#[test]
fn a_missing_file_is_empty() {
    let dir = TempDir::new("reg");
    let reg = files(&dir, "machines.json");
    assert_eq!(reg.all(), Vec::<Machine>::new());
    assert_eq!(reg.load(), RegistryState::Missing);
}

#[test]
fn bad_json_is_empty_not_a_crash() {
    let dir = TempDir::new("reg");
    write(&dir.join("machines.json"), "{ not json");
    assert_eq!(files(&dir, "machines.json").all(), Vec::<Machine>::new());
}

#[test]
fn python_defaults_when_the_key_is_missing() {
    let dir = TempDir::new("reg");
    write(
        &dir.join("machines.json"),
        r#"[{"name":"m1","host":"m1","user":"sondoan","sdPath":"/x/sd.py","fileVault":true,"repos":[]}]"#,
    );
    assert_eq!(files(&dir, "machines.json").all()[0].python(), "python3");
}

// MARK: machines.json v2 and the one-time migration

/// Plan C's doc-comment example, as the v1 file a user may already have.
const V1_TEXT: &str = r#"[
  {"name": "m1", "host": "m1", "user": "sondoan", "python": "python3",
   "sdPath": "/Users/sondoan/sd/sd.py", "fileVault": true,
   "repos": [{"github": "dtthaison/lingeva-mono",
              "localPath": "/Users/sondoan/go/src/lingeva/lingeva-mono",
              "checkCommand": "just check-affected"}]},
  {"name": "vps-sing-1", "host": "vps-sing-1", "user": "agent", "python": "python3",
   "sdPath": "/home/agent/sd/sd.py", "fileVault": false,
   "repos": [{"github": "dtthaison/lingeva-mono",
              "localPath": "/Users/sondoan/go/src/lingeva/lingeva-mono",
              "checkCommand": "just check-affected"}]}
]"#;

const V1_OTHER: &str =
    r#"[{"name":"z","host":"z","user":"u","sdPath":"/z/sd.py","fileVault":false,"repos":[]}]"#;

#[test]
fn v1_migrates_every_field_and_keeps_a_byte_identical_backup() {
    let dir = TempDir::new("reg");
    let file = dir.join("machines.json");
    write(&file, V1_TEXT);
    let reg = files(&dir, "machines.json");

    let RegistryState::Loaded { machines, migrated } = reg.load() else {
        panic!("not loaded");
    };
    assert!(migrated);
    assert_eq!(
        machines.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        vec!["m1", "vps-sing-1"]
    );
    let (m1, vps) = (&machines[0], &machines[1]);
    assert_eq!((m1.host.as_str(), m1.user.as_str()), ("m1", "sondoan"));
    assert_eq!(m1.python(), "python3");
    assert_eq!(m1.sd_path(), "/Users/sondoan/sd/sd.py");
    assert!(m1.file_vault);
    assert_eq!(m1.repos(), [repo()]);
    assert_eq!(
        (vps.host.as_str(), vps.user.as_str()),
        ("vps-sing-1", "agent")
    );
    assert_eq!(vps.sd_path(), "/home/agent/sd/sd.py");
    assert!(!vps.file_vault);
    for m in &machines {
        assert_eq!(m.jobs.len(), 1);
        assert_eq!(m.jobs[0].kind, JobKind::SdWorker);
        assert!(m.sd_worker().unwrap().kinds.is_empty());
        assert_eq!(m.claude_secret, "claude-worker");
        assert_eq!(m.admin(), AdminAccess::Sudo);
    }
    assert_eq!(m1.sd_worker().unwrap().issues_secret, "m1-github-issues");
    assert_eq!(
        vps.sd_worker().unwrap().issues_secret,
        "vps-sing-1-github-issues"
    );

    assert_eq!(
        std::fs::read(backup_path(&file)).unwrap(),
        V1_TEXT.as_bytes()
    );
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"version\": 2"), "{text}");
    assert_eq!(reg.all(), machines);
}

#[test]
fn migration_runs_once() {
    let dir = TempDir::new("reg");
    let file = dir.join("machines.json");
    write(&file, V1_TEXT);
    let reg = files(&dir, "machines.json");
    let RegistryState::Loaded {
        machines: first,
        migrated: true,
    } = reg.load()
    else {
        panic!("first load");
    };
    assert_eq!(
        reg.load(),
        RegistryState::Loaded {
            machines: first,
            migrated: false
        }
    );
    assert_eq!(
        std::fs::read(backup_path(&file)).unwrap(),
        V1_TEXT.as_bytes()
    );
    assert_eq!(
        names(&dir),
        vec!["machines.json", "machines.json.v1-backup"]
    );
}

#[test]
fn a_second_v1_file_gets_its_own_backup() {
    let dir = TempDir::new("reg");
    let file = dir.join("machines.json");
    write(&file, V1_TEXT);
    let reg = files(&dir, "machines.json");
    reg.load();
    write(&file, V1_OTHER);

    let RegistryState::Loaded { machines, migrated } = reg.load() else {
        panic!("second load");
    };
    assert!(migrated);
    assert_eq!(
        machines.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        vec!["z"]
    );
    assert_eq!(
        std::fs::read(backup_path(&file)).unwrap(),
        V1_TEXT.as_bytes()
    );
    let extra: Vec<String> = names(&dir)
        .into_iter()
        .filter(|n| n.starts_with("machines.json.v1-backup-"))
        .collect();
    assert_eq!(extra.len(), 1);
    let digits = extra[0].trim_start_matches("machines.json.v1-backup-");
    assert!(!digits.is_empty());
    assert!(
        digits.chars().all(|c| c.is_ascii_digit() || c == '-'),
        "{}",
        extra[0]
    );
    assert_eq!(
        std::fs::read(dir.join(&extra[0])).unwrap(),
        V1_OTHER.as_bytes()
    );
}

#[cfg(unix)]
fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(unix)]
#[test]
fn a_backup_that_cannot_be_written_stops_the_migration() {
    use std::os::unix::fs::PermissionsExt;
    if is_root() {
        return;
    }
    let dir = TempDir::new("reg");
    let file = dir.join("machines.json");
    write(&file, V1_TEXT);
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let reg = files(&dir, "machines.json");
    let RegistryState::Loaded { machines, migrated } = reg.load() else {
        panic!("not loaded");
    };
    assert!(!migrated);
    assert_eq!(
        machines.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        vec!["m1", "vps-sing-1"]
    );
    assert_eq!(std::fs::read(&file).unwrap(), V1_TEXT.as_bytes());
    assert!(!backup_path(&file).exists());
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_is_never_overwritten() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("reg");
    let bad = dir.join("bad.json");
    write(&bad, "{ not json");
    let reg = RegistryFiles::single(&bad);
    assert!(matches!(reg.load(), RegistryState::Unreadable(_)));
    assert!(!reg.save(&sample()));
    assert_eq!(std::fs::read(&bad).unwrap(), b"{ not json");

    if !is_root() {
        let locked = dir.join("locked.json");
        write(&locked, V1_TEXT);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let reg = RegistryFiles::single(&locked);
        assert!(matches!(reg.load(), RegistryState::Unreadable(_)));
        assert_eq!(reg.all(), Vec::<Machine>::new());
        assert!(!reg.save(&sample()));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(std::fs::read(&locked).unwrap(), V1_TEXT.as_bytes());
        assert_eq!(names(&dir), vec!["bad.json", "locked.json"]);
    }
}

#[test]
fn unknown_version_or_job_kind_is_unreadable() {
    let dir = TempDir::new("reg");
    let v3 = (dir.join("v3.json"), r#"{"version":3,"machines":[]}"#);
    let kind = (
        dir.join("kind.json"),
        r#"{"version":2,"machines":[{"name":"m","host":"m","user":"u","jobs":[{"kind":"triage-bot"}]}]}"#,
    );
    for (path, text) in [&v3, &kind] {
        write(path, text);
        let reg = RegistryFiles::single(path);
        assert!(
            matches!(reg.load(), RegistryState::Unreadable(_)),
            "{}",
            path.display()
        );
        assert!(!reg.save(&sample()));
        assert_eq!(std::fs::read(path).unwrap(), text.as_bytes());
    }
    assert_eq!(names(&dir), vec!["kind.json", "v3.json"]);
}

/// A Linux machine's admin can't be the worker account, so a file that doesn't say defaults it to
/// root over SSH. A Mac keeps sudo, and what a file says is kept.
#[test]
fn a_linux_machine_without_an_admin_key_defaults_to_root_over_ssh() {
    let dir = TempDir::new("reg");
    write(
        &dir.join("machines.json"),
        r#"{"version":2,"machines":[{"name":"a","host":"a","user":"agent","os":"linux"},{"name":"b","host":"b","user":"u","os":"macos"},{"name":"c","host":"c","user":"u"},{"name":"d","host":"d","user":"u","os":"linux","admin":"sudo"}]}"#,
    );
    let machines = files(&dir, "machines.json").all();
    assert_eq!(
        machines.iter().map(|m| m.admin()).collect::<Vec<_>>(),
        vec![
            AdminAccess::RootSsh,
            AdminAccess::Sudo,
            AdminAccess::Sudo,
            AdminAccess::Sudo
        ]
    );
}

#[test]
fn v2_fields_may_be_missing_except_name_host_user() {
    let dir = TempDir::new("reg");
    let file = dir.join("machines.json");
    let reg = RegistryFiles::single(&file);
    write(
        &file,
        r#"{"version":2,"machines":[{"name":"vps","host":"vps.tail","user":"agent"}]}"#,
    );
    assert_eq!(
        reg.load(),
        RegistryState::Loaded {
            machines: vec![Machine::new("vps", "vps.tail", "agent")],
            migrated: false
        }
    );
    let m = reg.machine("vps").unwrap();
    assert_eq!(m.python(), "python3");
    assert_eq!(m.sd_path(), "");
    assert!(m.repos().is_empty());
    assert!(m.sd_worker().is_none());
    assert_eq!(m.claude_secret, "claude-worker");
    assert_eq!(m.admin(), AdminAccess::Sudo);
    // A job with no settings in it gets the defaults, with the secret named after the machine.
    write(
        &file,
        r#"{"version":2,"machines":[{"name":"vps","host":"h","user":"u","jobs":[{"kind":"sd-worker","sdWorker":{}}]}]}"#,
    );
    let m = reg.machine("vps").unwrap();
    assert_eq!(m.sd_worker().unwrap().issues_secret, "vps-github-issues");
    assert_eq!(m.sd_worker().unwrap().python, "python3");
}

#[test]
fn save_writes_when_missing_and_round_trips_v2() {
    let dir = TempDir::new("reg");
    let file = dir.join("machines.json");
    let reg = RegistryFiles::single(&file);
    let mut worker = SdWorkerSettings::new("vps-sing-1");
    worker.python = "/usr/bin/python3.12".into();
    worker.sd_path = "/home/agent/sd/sd.py".into();
    worker.repos = vec![MachineRepo {
        github: "dtthaison/lingeva-mono".into(),
        local_path: "/Users/me/lingeva".into(),
        check_command: "just check".into(),
        machine_path: Some("~/src/lingeva".into()),
        verify_skill: Some(".claude/skills/verify-lingeva/SKILL.md".into()),
    }];
    worker.kinds = vec!["backend".into()];
    worker.health_url = Some("http://127.0.0.1:8080/health".into());
    worker.token_expires = Some("2027-02-01".into());
    let mut m = Machine::new("vps-sing-1", "vps-sing-1", "agent");
    m.os = Some(MachineOs::Linux);
    m.set_admin(AdminAccess::RootSsh);
    m.abilities = Some(MachineAbilities {
        arch: Some("x86_64".into()),
        os_version: Some("Ubuntu 24.04".into()),
        home: Some("/home/agent".into()),
        docker: Some(DockerState::Rootless),
        free_disk_gb: Some(42),
        ..MachineAbilities::default()
    });
    m.set_sd_worker(Some(worker));
    assert!(reg.save(std::slice::from_ref(&m)));
    assert_eq!(
        reg.load(),
        RegistryState::Loaded {
            machines: vec![m.clone()],
            migrated: false
        }
    );
    assert_eq!(reg.all(), vec![m.clone()]);
    assert_eq!(m.python(), "/usr/bin/python3.12");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"version\": 2"));
    // keys are sorted: admin before claudeSecret
    assert!(text.find("\"admin\"").unwrap() < text.find("\"claudeSecret\"").unwrap());
    assert!(
        text.contains("\"root-ssh\"")
            && text.contains("\"sd-worker\"")
            && text.contains("\"linux\"")
    );
    // A saved v2 file is saved again without a backup.
    assert!(reg.save(&[m]));
    assert!(!backup_path(&file).exists());
}

#[test]
fn the_sd_worker_setter_adds_replaces_and_removes_the_job() {
    let mut m = Machine::new("m", "m", "u");
    assert!(m.sd_worker().is_none());
    assert_eq!(m.python(), "python3");
    assert_eq!(m.sd_path(), "");
    let mut s1 = SdWorkerSettings::new("m");
    s1.python = "p1".into();
    s1.sd_path = "/a/sd.py".into();
    m.set_sd_worker(Some(s1.clone()));
    assert_eq!(m.jobs.len(), 1);
    assert_eq!(m.sd_path(), "/a/sd.py");
    let mut s2 = s1;
    s2.sd_path = "/b/sd.py".into();
    m.set_sd_worker(Some(s2));
    assert_eq!(m.jobs.len(), 1);
    assert_eq!(m.sd_path(), "/b/sd.py");
    m.set_sd_worker(None);
    assert!(m.jobs.is_empty());
}

// MARK: the Rust tool's own file next to the Swift app's

/// Until handover the Swift app's file is only ever read: a v1 file there migrates into the tool's
/// own file, and the Swift file's bytes stay as they were.
#[test]
fn the_swift_apps_file_is_read_and_never_written() {
    let dir = TempDir::new("reg");
    let swift = dir.join("swift/machines.json");
    std::fs::create_dir_all(swift.parent().unwrap()).unwrap();
    write(&swift, V1_TEXT);
    let own = dir.join("own/machines.json");
    let reg = RegistryFiles {
        own: own.clone(),
        swift: Some(swift.clone()),
    };
    assert_eq!(reg.all().len(), 2, "read from the Swift app's file");
    assert!(own.exists(), "migrated into the tool's own file");
    assert_eq!(std::fs::read(&swift).unwrap(), V1_TEXT.as_bytes());
    assert_eq!(
        std::fs::read(backup_path(&own)).unwrap(),
        V1_TEXT.as_bytes()
    );
    assert!(reg.save(&sample()));
    assert_eq!(std::fs::read(&swift).unwrap(), V1_TEXT.as_bytes());
    assert_eq!(reg.all(), sample(), "the own file wins once it exists");
}
