use super::config_file::{check_enable, enable};
use super::hooks_file::{apply, check, Want};
use super::SIDECAR_ID;
use crate::install::file::MAX_BYTES;
use notchling_platform::paths::Os;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const BIN: &str = "/Applications/Notchling.app/Contents/MacOS/notchling";
const GOOD: &str = "{\"plugins\": {}}\n";

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-blocked-{name}-{}", std::process::id()));
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn mode(path: &Path, bits: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits)).unwrap();
}

type Block = (&'static str, fn(&Path));

fn blocks() -> Vec<Block> {
    vec![
        ("read-only", |p| {
            std::fs::write(p, GOOD).unwrap();
            mode(p, 0o444);
        }),
        ("too big", |p| {
            std::fs::write(p, " ".repeat(MAX_BYTES as usize + 1)).unwrap();
        }),
        ("dangling link", |p| {
            std::os::unix::fs::symlink(p.with_file_name("nowhere.json"), p).unwrap();
        }),
        ("a folder", |p| std::fs::create_dir(p).unwrap()),
        ("not utf-8", |p| {
            std::fs::write(p, [0x7b, 0xff, 0xfe, 0x7d]).unwrap()
        }),
        ("unreadable", |p| {
            std::fs::write(p, GOOD).unwrap();
            mode(p, 0o000);
        }),
    ]
}

fn unreadable_is_enforced(dir: &Path) -> bool {
    let probe = dir.join("probe");
    std::fs::write(&probe, "x").unwrap();
    mode(&probe, 0o000);
    let enforced = std::fs::read(&probe).is_err();
    mode(&probe, 0o644);
    enforced
}

#[test]
fn config_json_in_a_state_the_write_would_refuse_is_refused_before_anything_is_written() {
    for (name, make) in blocks() {
        let dir = folder("config");
        if name == "unreadable" && !unreadable_is_enforced(&dir) {
            continue;
        }
        let path = dir.join("config.json");
        make(&path);
        assert!(check_enable(&path, SIDECAR_ID).is_err(), "{name}");
        assert!(enable(&path, SIDECAR_ID, true, 1).is_err(), "{name}");
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));
        let backups = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("backup"))
            .count();
        assert_eq!(backups, 0, "{name}");
    }
}

#[test]
fn hooks_json_in_a_state_the_write_would_refuse_is_refused_before_anything_is_written() {
    for (name, make) in blocks() {
        let dir = folder("hooks");
        if name == "unreadable" && !unreadable_is_enforced(&dir) {
            continue;
        }
        let path = dir.join("hooks.json");
        make(&path);
        let bin = Path::new(BIN);
        assert!(
            check(&path, bin, Os::current(), Want::Install).is_err(),
            "{name}"
        );
        assert!(
            apply(&path, bin, Os::current(), Want::Install, 1).is_err(),
            "{name}"
        );
    }
}

#[test]
fn a_folder_that_cannot_be_written_to_is_refused_for_both_files() {
    let dir = folder("nowrite");
    let config = dir.join("config.json");
    std::fs::write(&config, GOOD).unwrap();
    let hooks = dir.join("hooks.json");
    std::fs::write(&hooks, GOOD).unwrap();
    mode(&dir, 0o555);
    let blocked = std::fs::File::create(dir.join("probe")).is_err();
    let config_refused = check_enable(&config, SIDECAR_ID).is_err();
    let hooks_refused = check(&hooks, Path::new(BIN), Os::current(), Want::Install).is_err();
    mode(&dir, 0o755);
    if blocked {
        assert!(config_refused);
        assert!(hooks_refused);
    }
}

#[test]
fn files_that_can_be_written_pass_the_check_and_it_leaves_nothing_behind() {
    let dir = folder("fine");
    let config = dir.join("config.json");
    std::fs::write(&config, GOOD).unwrap();
    let hooks = dir.join("hooks.json");
    assert!(check_enable(&config, SIDECAR_ID).is_ok());
    assert!(check(&hooks, Path::new(BIN), Os::current(), Want::Install).is_ok());
    std::fs::write(&hooks, GOOD).unwrap();
    assert!(check(&hooks, Path::new(BIN), Os::current(), Want::Install).is_ok());
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["config.json", "hooks.json"]);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), GOOD);
}
