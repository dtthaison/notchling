//! The registry: `machines.json` v2 kept as a document in `notchling.db`, the one-time v1
//! migration, the old file moving in, and what is never overwritten (Swift `MachinesRegistryTests`).
//! Files live in a private temp folder; nothing real is read.

use super::fakes::TempDir;
use super::registry::*;
use crate::store;
use std::path::{Path, PathBuf};

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

/// A registry over the data folder `dir/<tag>`, its old own file there as `machines.json`, no
/// Swift file.
fn registry(dir: &TempDir, tag: &str) -> RegistryFiles {
    RegistryFiles::single(&dir.join(tag))
}

/// The same, with the old own file's text written first.
fn with_old_file(dir: &TempDir, tag: &str, text: &str) -> RegistryFiles {
    let reg = registry(dir, tag);
    write(&reg.old_own, text);
    reg
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The files in `dir`, without the database's.
fn names(dir: &Path) -> Vec<String> {
    let mut list: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with(store::DB_NAME))
        .collect();
    list.sort();
    list
}

fn doc(reg: &RegistryFiles) -> Option<String> {
    store::doc(&reg.data_dir, REGISTRY_DOC).unwrap()
}

fn machine_names(machines: &[Machine]) -> Vec<&str> {
    machines.iter().map(|m| m.name.as_str()).collect()
}

#[test]
fn registry_round_trip() {
    let dir = TempDir::new("reg");
    let reg = registry(&dir, "data");
    assert!(reg.save(&sample()));
    assert_eq!(reg.all(), sample());
    assert_eq!(reg.machine("vps-sing-1").unwrap().user, "agent");
    assert_eq!(reg.machine("m1").unwrap().python(), "python3");
    assert_eq!(reg.machine("nope"), None);
    assert!(!reg.old_own.exists(), "no file is written");
}

#[test]
fn a_missing_registry_is_empty_and_reading_makes_no_database() {
    let dir = TempDir::new("reg");
    let reg = registry(&dir, "data");
    assert_eq!(reg.all(), Vec::<Machine>::new());
    assert_eq!(reg.load(), RegistryState::Missing);
    assert!(!dir.join("data").exists());
}

#[test]
fn bad_json_is_empty_not_a_crash() {
    let dir = TempDir::new("reg");
    let reg = with_old_file(&dir, "data", "{ not json");
    assert_eq!(reg.all(), Vec::<Machine>::new());
}

#[test]
fn python_defaults_when_the_key_is_missing() {
    let dir = TempDir::new("reg");
    let reg = with_old_file(
        &dir,
        "data",
        r#"[{"name":"m1","host":"m1","user":"sondoan","sdPath":"/x/sd.py","fileVault":true,"repos":[]}]"#,
    );
    assert_eq!(reg.all()[0].python(), "python3");
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
    let reg = with_old_file(&dir, "data", V1_TEXT);

    let RegistryState::Loaded { machines, migrated } = reg.load() else {
        panic!("not loaded");
    };
    assert!(migrated);
    assert_eq!(machine_names(&machines), vec!["m1", "vps-sing-1"]);
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
        std::fs::read(backup_path(&reg.old_own)).unwrap(),
        V1_TEXT.as_bytes()
    );
    let text = doc(&reg).unwrap();
    assert!(text.contains("\"version\": 2"), "{text}");
    assert!(
        !reg.old_own.exists(),
        "the old file moved into the database"
    );
    assert_eq!(reg.all(), machines);
}

#[test]
fn migration_runs_once() {
    let dir = TempDir::new("reg");
    let reg = with_old_file(&dir, "data", V1_TEXT);
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
        std::fs::read(backup_path(&reg.old_own)).unwrap(),
        V1_TEXT.as_bytes()
    );
    assert_eq!(names(&dir.join("data")), vec!["machines.json.v1-backup"]);
}

#[test]
fn a_second_v1_registry_gets_its_own_backup() {
    let dir = TempDir::new("reg");
    let reg = with_old_file(&dir, "data", V1_TEXT);
    reg.load();
    // the registry is gone (a removed document), and another v1 file turns up
    store::update_doc(&reg.data_dir, REGISTRY_DOC, |_| Ok(None)).unwrap();
    write(&reg.old_own, V1_OTHER);

    let RegistryState::Loaded { machines, migrated } = reg.load() else {
        panic!("second load");
    };
    assert!(migrated);
    assert_eq!(machine_names(&machines), vec!["z"]);
    let backup = backup_path(&reg.old_own);
    assert_eq!(std::fs::read(&backup).unwrap(), V1_TEXT.as_bytes());
    let prefix = backup.file_name().unwrap().to_string_lossy().into_owned() + "-";
    let extra: Vec<String> = names(&dir.join("data"))
        .into_iter()
        .filter(|n| n.starts_with(&prefix))
        .collect();
    assert_eq!(extra.len(), 1);
    let digits = extra[0].trim_start_matches(&prefix);
    assert!(!digits.is_empty());
    assert!(
        digits.chars().all(|c| c.is_ascii_digit() || c == '-'),
        "{}",
        extra[0]
    );
    assert_eq!(
        std::fs::read(dir.join("data").join(&extra[0])).unwrap(),
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
    let reg = with_old_file(&dir, "data", V1_TEXT);
    let data = dir.join("data");
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o500)).unwrap();
    let RegistryState::Loaded { machines, migrated } = reg.load() else {
        panic!("not loaded");
    };
    assert!(!migrated);
    assert_eq!(machine_names(&machines), vec!["m1", "vps-sing-1"]);
    assert_eq!(std::fs::read(&reg.old_own).unwrap(), V1_TEXT.as_bytes());
    assert!(!backup_path(&reg.old_own).exists());
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(unix)]
#[test]
fn an_unreadable_registry_is_never_overwritten() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("reg");
    let reg = with_old_file(&dir, "bad", "{ not json");
    assert!(matches!(reg.load(), RegistryState::Unreadable(_)));
    assert!(!reg.save(&sample()));
    assert_eq!(std::fs::read(&reg.old_own).unwrap(), b"{ not json");
    assert_eq!(doc(&reg), None);
    assert_eq!(names(&dir.join("bad")), vec!["machines.json"]);
    assert!(!store::exists(&reg.data_dir), "refusing made no database");

    if !is_root() {
        let reg = with_old_file(&dir, "locked", V1_TEXT);
        std::fs::set_permissions(&reg.old_own, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(matches!(reg.load(), RegistryState::Unreadable(_)));
        assert_eq!(reg.all(), Vec::<Machine>::new());
        assert!(!reg.save(&sample()));
        std::fs::set_permissions(&reg.old_own, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(std::fs::read(&reg.old_own).unwrap(), V1_TEXT.as_bytes());
        assert_eq!(names(&dir.join("locked")), vec!["machines.json"]);
    }
}

#[test]
fn unknown_version_or_job_kind_is_unreadable() {
    let dir = TempDir::new("reg");
    let cases = [
        ("v3", r#"{"version":3,"machines":[]}"#),
        (
            "kind",
            r#"{"version":2,"machines":[{"name":"m","host":"m","user":"u","jobs":[{"kind":"triage-bot"}]}]}"#,
        ),
    ];
    for (tag, text) in cases {
        let reg = with_old_file(&dir, tag, text);
        assert!(matches!(reg.load(), RegistryState::Unreadable(_)), "{tag}");
        assert!(!reg.save(&sample()));
        assert_eq!(std::fs::read(&reg.old_own).unwrap(), text.as_bytes());
        assert_eq!(doc(&reg), None, "{tag}");
    }
}

/// A Linux machine's admin can't be the worker account, so a file that doesn't say defaults it to
/// root over SSH. A Mac keeps sudo, and what a file says is kept.
#[test]
fn a_linux_machine_without_an_admin_key_defaults_to_root_over_ssh() {
    let dir = TempDir::new("reg");
    let reg = with_old_file(
        &dir,
        "data",
        r#"{"version":2,"machines":[{"name":"a","host":"a","user":"agent","os":"linux"},{"name":"b","host":"b","user":"u","os":"macos"},{"name":"c","host":"c","user":"u"},{"name":"d","host":"d","user":"u","os":"linux","admin":"sudo"}]}"#,
    );
    let machines = reg.all();
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
    let reg = with_old_file(
        &dir,
        "plain",
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
    let reg = with_old_file(
        &dir,
        "job",
        r#"{"version":2,"machines":[{"name":"vps","host":"h","user":"u","jobs":[{"kind":"sd-worker","sdWorker":{}}]}]}"#,
    );
    let m = reg.machine("vps").unwrap();
    assert_eq!(m.sd_worker().unwrap().issues_secret, "vps-github-issues");
    assert_eq!(m.sd_worker().unwrap().python, "python3");
}

#[test]
fn save_writes_when_missing_and_round_trips_v2() {
    let dir = TempDir::new("reg");
    let reg = registry(&dir, "data");
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
    let text = doc(&reg).unwrap();
    assert!(text.contains("\"version\": 2"));
    // keys are sorted: admin before claudeSecret
    assert!(text.find("\"admin\"").unwrap() < text.find("\"claudeSecret\"").unwrap());
    assert!(
        text.contains("\"root-ssh\"")
            && text.contains("\"sd-worker\"")
            && text.contains("\"linux\"")
    );
    // A saved v2 registry is saved again without a backup.
    assert!(reg.save(&[m]));
    assert!(!backup_path(&reg.old_own).exists());
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

// MARK: the Rust tool's own copy next to the Swift app's file

/// A registry with a Swift file as well, in `dir/swift/machines.json`.
fn with_swift(dir: &TempDir, text: &str) -> (RegistryFiles, PathBuf) {
    let swift = dir.join("swift/machines.json");
    write(&swift, text);
    let reg = RegistryFiles {
        data_dir: dir.join("data"),
        old_own: dir.join("data/daemon/machines/machines.json"),
        swift: Some(swift.clone()),
        read_only: false,
    };
    (reg, swift)
}

/// Until handover the Swift app's file is only ever read: a v1 file there migrates into the tool's
/// own document, and the Swift file's bytes stay as they were.
#[test]
fn the_swift_apps_file_is_read_and_never_written() {
    let dir = TempDir::new("reg");
    let (reg, swift) = with_swift(&dir, V1_TEXT);
    assert_eq!(reg.source(), swift);
    assert_eq!(reg.all().len(), 2, "read from the Swift app's file");
    assert!(doc(&reg).is_some(), "migrated into the tool's own document");
    assert_eq!(reg.source(), store::db_path(&reg.data_dir));
    assert_eq!(std::fs::read(&swift).unwrap(), V1_TEXT.as_bytes());
    assert_eq!(
        std::fs::read(backup_path(&reg.old_own)).unwrap(),
        V1_TEXT.as_bytes()
    );
    assert!(reg.save(&sample()));
    assert_eq!(std::fs::read(&swift).unwrap(), V1_TEXT.as_bytes());
    assert_eq!(reg.all(), sample(), "the document wins once it exists");
}

#[test]
fn a_v2_swift_file_is_read_and_saving_starts_the_documents() {
    let dir = TempDir::new("reg");
    let (reg, swift) = with_swift(
        &dir,
        r#"{"version":2,"machines":[{"name":"s","host":"s","user":"u"}]}"#,
    );
    assert_eq!(machine_names(&reg.all()), vec!["s"]);
    assert_eq!(doc(&reg), None, "reading wrote nothing");
    assert!(reg.save(&sample()));
    assert_eq!(machine_names(&reg.all()), vec!["m1", "vps-sing-1"]);
    assert!(std::fs::read_to_string(swift).unwrap().contains("\"s\""));
}

// MARK: the old own file moving into the database

const OLD_V2: &str = r#"{"version":2,"machines":[{"name":"old","host":"old","user":"u"}]}"#;

#[test]
fn a_valid_old_own_file_moves_in_once() {
    let dir = TempDir::new("reg");
    let reg = with_old_file(&dir, "data", OLD_V2);
    assert_eq!(machine_names(&reg.all()), vec!["old"]);
    assert_eq!(doc(&reg).as_deref(), Some(OLD_V2));
    assert!(!reg.old_own.exists());
    assert!(!backup_path(&reg.old_own).exists());
    assert_eq!(machine_names(&reg.all()), vec!["old"]);
}

#[test]
fn an_unreadable_old_own_file_stays_and_is_read_as_the_source() {
    let dir = TempDir::new("reg");
    let (reg, swift) = with_swift(&dir, V1_TEXT);
    write(&reg.old_own, "{ not json");
    // the broken own file shadows the Swift app's, as the broken own file did before
    assert!(matches!(reg.load(), RegistryState::Unreadable(_)));
    assert!(!reg.save(&sample()));
    assert_eq!(std::fs::read(&reg.old_own).unwrap(), b"{ not json");
    assert_eq!(doc(&reg), None);
    assert_eq!(reg.source(), reg.old_own);
    assert_eq!(std::fs::read(swift).unwrap(), V1_TEXT.as_bytes());
}

#[test]
fn an_existing_document_wins_over_the_old_own_file() {
    let dir = TempDir::new("reg");
    let reg = with_old_file(&dir, "data", OLD_V2);
    let other = RegistryFiles {
        old_own: dir.join("data/other.json"),
        ..reg.clone()
    };
    assert!(other.save(&sample()));
    assert_eq!(reg.all(), sample(), "the document, not the file");
    assert!(
        !reg.old_own.exists(),
        "the file is dropped, its content lost"
    );

    // a broken old file doesn't shadow a document either, and stays
    write(&reg.old_own, "{ not json");
    assert_eq!(reg.all(), sample());
    assert!(reg.save(&sample()[..1]));
    assert_eq!(machine_names(&reg.all()), vec!["m1"]);
    assert!(reg.old_own.exists());
}

#[test]
fn a_render_never_imports_migrates_or_saves() {
    let dir = TempDir::new("reg");
    let (mut reg, swift) = with_swift(&dir, V1_TEXT);
    reg.read_only = true;
    write(&reg.old_own, OLD_V2);
    assert_eq!(machine_names(&reg.all()), vec!["old"], "reads the old file");
    assert!(!store::exists(&reg.data_dir));
    assert!(reg.old_own.exists(), "not imported");
    assert!(!reg.save(&sample()));

    std::fs::remove_file(&reg.old_own).unwrap();
    let RegistryState::Loaded { machines, migrated } = reg.load() else {
        panic!("not loaded");
    };
    assert!(!migrated, "a v1 file is not migrated");
    assert_eq!(machine_names(&machines), vec!["m1", "vps-sing-1"]);
    assert!(!store::exists(&reg.data_dir));
    assert!(!backup_path(&reg.old_own).exists());
    assert_eq!(std::fs::read(swift).unwrap(), V1_TEXT.as_bytes());
}

// MARK: writers at once

#[test]
fn writers_at_once_never_corrupt_the_registry() {
    let dir = TempDir::new("reg");
    let reg = registry(&dir, "data");
    let lists: Vec<Vec<Machine>> = (1..=4)
        .map(|n| {
            (0..n)
                .map(|i| Machine::new(&format!("m{n}-{i}"), "h", "u"))
                .collect()
        })
        .collect();
    std::thread::scope(|scope| {
        for list in &lists {
            let reg = reg.clone();
            scope.spawn(move || {
                for _ in 0..10 {
                    assert!(reg.save(list));
                    assert!(matches!(reg.load(), RegistryState::Loaded { .. }));
                }
            });
        }
    });
    let last = reg.all();
    assert!(lists.contains(&last), "one writer's whole list won");
}

#[test]
fn two_first_loads_of_a_v1_registry_migrate_it_once() {
    let dir = TempDir::new("reg");
    let reg = with_old_file(&dir, "data", V1_TEXT);
    let results: Vec<RegistryState> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let reg = reg.clone();
                scope.spawn(move || reg.load())
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for state in &results {
        let RegistryState::Loaded { machines, .. } = state else {
            panic!("not loaded: {state:?}");
        };
        assert_eq!(machine_names(machines), vec!["m1", "vps-sing-1"]);
    }
    assert!(doc(&reg).unwrap().contains("\"version\": 2"));
    assert_eq!(
        std::fs::read(backup_path(&reg.old_own)).unwrap(),
        V1_TEXT.as_bytes()
    );
    assert_eq!(reg.all().len(), 2);
}
