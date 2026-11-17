use super::*;
use std::sync::Arc;

const STATE: &str = include_str!("../../tests/fixtures/job-fan/state.json");

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("notchling-job-fan-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("folder");
    dir
}

#[test]
fn the_fan_lists_running_shells_and_agents_only() {
    let fan = parse(STATE.as_bytes());
    let ids: Vec<&str> = fan.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["bshell0001", "aagent00000000001"]);
    assert_eq!(fan[0].kind, FanKind::Shell);
    assert_eq!(fan[0].label, "sleep 600");
    assert_eq!(fan[0].started, Some(1_794_000_000.0));
    assert_eq!(fan[1].kind, FanKind::Agent);
}

#[test]
fn a_state_file_with_no_fan_or_bad_json_lists_nothing() {
    assert!(parse(br#"{"state":"done"}"#).is_empty());
    assert!(parse(br#"{"fan":"x"}"#).is_empty());
    assert!(parse(b"not json").is_empty());
    assert!(parse(b"").is_empty());
}

#[test]
fn the_state_file_is_under_the_jobs_folder_of_the_config_dir() {
    let dir = std::path::Path::new("/c");
    assert_eq!(
        state_path(dir, "9f3883da"),
        Some(std::path::PathBuf::from("/c/jobs/9f3883da/state.json"))
    );
    assert_eq!(state_path(dir, ""), None);
    assert_eq!(state_path(dir, "../x"), None);
    assert_eq!(state_path(dir, "a/b"), None);
}

#[test]
fn an_unchanged_file_is_not_read_again_and_a_changed_one_is() {
    let dir = scratch("cache");
    let file = dir.join("state.json");
    let mut reader = FanReader::default();
    assert!(reader.read(&file).is_empty(), "missing file");
    std::fs::write(&file, STATE).expect("write");
    let first = reader.read(&file);
    assert_eq!(first.len(), 2);
    assert!(
        Arc::ptr_eq(&first, &reader.read(&file)),
        "same size and mtime"
    );
    std::fs::write(&file, br#"{"fan":[]}"#).expect("rewrite");
    assert!(reader.read(&file).is_empty());
    std::fs::remove_file(&file).expect("remove");
    assert!(reader.read(&file).is_empty(), "gone again");
    let _ = std::fs::remove_dir_all(&dir);
}
