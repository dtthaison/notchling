use super::*;
use notchling_engine::control::{FakeLauncher, LaunchError, LaunchOutput};
use std::time::Duration;

fn probe(os: &'static str, fake: &Arc<FakeLauncher>) -> HostProbe {
    HostProbe::new(
        fake.clone(),
        Host {
            os,
            home: PathBuf::from("/home/son"),
            user: "son".to_string(),
        },
        Box::new(|_| Some(123_456)),
    )
}

const LUKS_HOME: &str = r#"{"blockdevices":[
  {"name":"nvme0n1","type":"disk","mountpoint":null,"children":[
    {"name":"nvme0n1p1","type":"part","mountpoint":"/boot"},
    {"name":"nvme0n1p2","type":"part","mountpoint":null,"children":[
      {"name":"cryptroot","type":"crypt","mountpoint":"/"}
    ]}
  ]}
]}"#;

const PLAIN_HOME: &str = r#"{"blockdevices":[
  {"name":"sda","type":"disk","mountpoint":null,"children":[
    {"name":"sda1","type":"part","mountpoint":"/"}
  ]}
]}"#;

const CRYPT_HOME_ON_SEPARATE_MOUNT: &str = r#"{"blockdevices":[
  {"name":"sda","type":"disk","mountpoint":null,"children":[
    {"name":"sda1","type":"part","mountpoint":"/"},
    {"name":"sda2","type":"part","mountpoint":null,"children":[
      {"name":"crypthome","type":"crypt","mountpoint":"/home"}
    ]}
  ]}
]}"#;

const LVM_ON_CRYPT: &str = r#"{"blockdevices":[
  {"name":"sda","type":"disk","mountpoint":null,"children":[
    {"name":"sda2","type":"part","mountpoint":null,"children":[
      {"name":"cryptlvm","type":"crypt","mountpoint":null,"children":[
        {"name":"vg-root","type":"lvm","mountpoint":"/"}
      ]}
    ]}
  ]}
]}"#;

const BOOT_ONLY_CRYPT_ELSEWHERE: &str = r#"{"blockdevices":[
  {"name":"sda","type":"disk","mountpoint":null,"children":[
    {"name":"sda1","type":"part","mountpoint":"/"},
    {"name":"sda2","type":"part","mountpoint":null,"children":[
      {"name":"cryptdata","type":"crypt","mountpoint":"/data"}
    ]}
  ]}
]}"#;

#[test]
fn filevault_on_passes() {
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok("FileVault is On.\n")));
    let facts = probe("macos", &fake).facts();
    assert_eq!(facts.encrypted, Some(true));
    assert_eq!(facts.home, "/home/son");
    assert_eq!(facts.user, "son");
}

#[test]
fn filevault_off_is_false_and_a_failed_probe_is_unknown() {
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok("FileVault is Off.\n")));
    assert_eq!(probe("macos", &fake).facts().encrypted, Some(false));
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Err(LaunchError::Timeout(5)));
    assert_eq!(probe("macos", &fake).facts().encrypted, None);
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok("Encryption in progress: 40%\n")));
    assert_eq!(probe("macos", &fake).facts().encrypted, None);
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::failed(1, "FileVault is On.")));
    assert_eq!(probe("macos", &fake).facts().encrypted, None);
}

#[test]
fn luks_home_passes() {
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok(LUKS_HOME)));
    assert_eq!(probe("linux", &fake).facts().encrypted, Some(true));
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok(CRYPT_HOME_ON_SEPARATE_MOUNT)));
    assert_eq!(probe("linux", &fake).facts().encrypted, Some(true));
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok(LVM_ON_CRYPT)));
    assert_eq!(probe("linux", &fake).facts().encrypted, Some(true));
}

#[test]
fn a_plain_home_is_not_encrypted_whatever_else_is() {
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok(PLAIN_HOME)));
    assert_eq!(probe("linux", &fake).facts().encrypted, Some(false));
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok(BOOT_ONLY_CRYPT_ELSEWHERE)));
    assert_eq!(probe("linux", &fake).facts().encrypted, Some(false));
}

#[test]
fn lsblk_noise_or_failure_is_unknown() {
    for out in [
        Ok(LaunchOutput::ok("not json")),
        Ok(LaunchOutput::ok(r#"{"blockdevices":[]}"#)),
        Ok(LaunchOutput::failed(127, "")),
        Err(LaunchError::Spawn("no lsblk".to_string())),
    ] {
        let fake = Arc::new(FakeLauncher::new());
        fake.push_output(out);
        assert_eq!(probe("linux", &fake).facts().encrypted, None);
    }
}

#[test]
fn host_facts_probe_runs_read_only_commands() {
    let fake = Arc::new(FakeLauncher::new());
    probe("macos", &fake).facts();
    probe("linux", &fake).facts();
    probe("windows", &fake).facts();
    let argv: Vec<Vec<String>> = fake.commands().into_iter().map(|c| c.argv).collect();
    assert_eq!(
        argv,
        vec![
            vec!["/usr/bin/fdesetup".to_string(), "status".to_string()],
            vec![
                "lsblk".to_string(),
                "-o".to_string(),
                "NAME,TYPE,MOUNTPOINT".to_string(),
                "-J".to_string()
            ],
        ]
    );
    for call in fake.calls() {
        let notchling_engine::control::Call::Run(_, timeout) = call else {
            panic!("the probe never uses a pty");
        };
        assert_eq!(timeout, Duration::from_secs(5));
    }
}

#[test]
fn facts_are_read_at_most_every_ten_minutes() {
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok("FileVault is On.")));
    let p = probe("macos", &fake);
    let first = p.facts();
    let second = p.facts();
    assert_eq!(first, second);
    assert_eq!(fake.commands().len(), 1);
    assert_eq!(EVERY, Duration::from_secs(600));
    let again = probe("macos", &fake).with_every(Duration::ZERO);
    again.facts();
    again.facts();
    assert_eq!(fake.commands().len(), 3);
}

#[test]
fn free_space_is_rounded_down_and_unknown_is_zero() {
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok("FileVault is On.")));
    assert_eq!(probe("macos", &fake).facts().free_mb, 123_392);
    let fake = Arc::new(FakeLauncher::new());
    let p = HostProbe::new(
        fake.clone(),
        Host {
            os: "macos",
            home: PathBuf::from("/h"),
            user: "u".to_string(),
        },
        Box::new(|_| None),
    );
    assert_eq!(p.facts().free_mb, 0);
}

#[cfg(unix)]
#[test]
fn the_real_free_space_reader_reports_something_for_a_real_folder() {
    let mb = free_mb_of(Path::new("/tmp"));
    assert!(mb.is_some_and(|mb| mb > 0));
}

#[test]
fn versions_carry_the_facts_and_old_peers_without_them_still_parse() {
    use notchling_engine::cluster::versions::Versions;
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok("FileVault is On.")));
    let mut v = crate::cluster_versions::own();
    v.host = Some(probe("macos", &fake).facts());
    let text = serde_json::to_string(&v).unwrap();
    let back: Versions = serde_json::from_str(&text).unwrap();
    assert_eq!(back, v);
    let mut old = serde_json::to_value(&v).unwrap();
    old.as_object_mut().unwrap().remove("host");
    let parsed: Versions = serde_json::from_value(old).unwrap();
    assert_eq!(parsed.host, None);
}
