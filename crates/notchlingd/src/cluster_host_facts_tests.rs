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

#[cfg(unix)]
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

#[cfg(unix)]
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

fn watched(fake: &Arc<FakeLauncher>, every: Duration, retry: Duration) -> HostWatch {
    HostWatch::spawn(Arc::new(probe("macos", fake)), every, retry)
}

fn until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("never happened: {what}");
}

#[test]
fn the_cadence_is_ten_minutes_and_a_minute_after_an_unknown_read() {
    assert_eq!(EVERY, Duration::from_secs(600));
    assert_eq!(RETRY, Duration::from_secs(60));
    assert_eq!(TIMEOUT, Duration::from_secs(5));
}

#[test]
fn a_known_read_is_not_repeated_before_its_cadence() {
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok("FileVault is On.")));
    let watch = watched(&fake, Duration::from_secs(3600), Duration::from_millis(5));
    until("the first read", || watch.facts().is_some());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fake.commands().len(), 1);
    assert_eq!(watch.facts().unwrap().encrypted, Some(true));
}

#[test]
fn an_unknown_read_is_retried_at_the_short_cadence() {
    let fake = Arc::new(FakeLauncher::new());
    fake.push_output(Ok(LaunchOutput::ok("odd")));
    fake.push_output(Ok(LaunchOutput::ok("FileVault is On.")));
    let watch = watched(&fake, Duration::from_secs(3600), Duration::from_millis(5));
    until("the retry found it", || {
        watch.facts().is_some_and(|f| f.encrypted == Some(true))
    });
    assert_eq!(fake.commands().len(), 2);
}

#[test]
fn nothing_is_known_until_the_first_read_and_a_slow_probe_never_waits_for_a_caller() {
    let fake = Arc::new(FakeLauncher::new());
    fake.set_delay(Duration::from_millis(400));
    fake.push_output(Ok(LaunchOutput::ok("FileVault is On.")));
    let started = std::time::Instant::now();
    let watch = watched(&fake, Duration::from_secs(3600), Duration::from_secs(3600));
    assert_eq!(watch.facts(), None);
    assert!(started.elapsed() < Duration::from_millis(200));
    until("the slow read lands", || watch.facts().is_some());
}

#[test]
fn dropping_the_watch_stops_the_reads() {
    let fake = Arc::new(FakeLauncher::new());
    let watch = watched(&fake, Duration::from_millis(5), Duration::from_millis(5));
    assert!(watch.running());
    until("a few reads", || fake.commands().len() >= 3);
    drop(watch);
    std::thread::sleep(Duration::from_millis(50));
    let after = fake.commands().len();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fake.commands().len(), after);
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
