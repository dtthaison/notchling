use super::hooks_file::{apply, change, desired, found, Found, Want};
use notchling_platform::paths::Os;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const BIN: &str = "/Applications/Notchling.app/Contents/MacOS/notchling";
const USERS: &str = "{\n  \"lint\": {\n    \"PostToolUse\": [\n      {\"matcher\": \"run_command\", \"hooks\": [{\"command\": \"./lint.sh\", \"timeout\": 10}]}\n    ]\n  },\n  \"reminder\": {\"enabled\": false, \"PreInvocation\": [{\"command\": \"./r.sh\"}]}\n}\n";

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-hooks-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn bin() -> PathBuf {
    PathBuf::from(BIN)
}

fn json_of(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn backups(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".notchling-backup"))
        .collect()
}

#[test]
fn the_entry_asks_before_tools_and_checks_for_a_stop_before_each_model_call() {
    let want = desired(&bin(), Os::Mac);
    let tool = &want["PreToolUse"][0];
    assert!(tool["matcher"].as_str().unwrap().contains("run_command"));
    assert_eq!(tool["hooks"][0]["timeout"], 120);
    assert_eq!(
        tool["hooks"][0]["command"],
        format!("'{BIN}' agy-hook pre-tool")
    );
    assert_eq!(
        want["PreInvocation"][0]["command"],
        format!("'{BIN}' agy-hook pre-invocation")
    );
}

#[test]
fn it_adds_only_its_entry_and_keeps_every_other_byte() {
    let out = change(USERS, &bin(), Os::Mac, Want::Install)
        .unwrap()
        .unwrap();
    assert!(out.contains("\"lint\""));
    assert!(out.contains("{\"matcher\": \"run_command\", \"hooks\": [{\"command\": \"./lint.sh\", \"timeout\": 10}]}"));
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["notchling"], desired(&bin(), Os::Mac));
    assert_eq!(value["reminder"]["enabled"], false);
    let removed = change(&out, &bin(), Os::Mac, Want::Remove)
        .unwrap()
        .unwrap();
    assert_eq!(removed, USERS);
}

#[test]
fn installing_twice_changes_nothing_the_second_time() {
    let dir = folder("twice");
    let path = dir.join("hooks.json");
    std::fs::write(&path, USERS).unwrap();
    let first = apply(&path, &bin(), Os::Mac, Want::Install, 1).unwrap();
    assert!(first.wrote);
    let text = std::fs::read_to_string(&path).unwrap();
    let second = apply(&path, &bin(), Os::Mac, Want::Install, 2).unwrap();
    assert!(!second.wrote);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert_eq!(backups(&dir).len(), 1);
}

#[test]
fn a_changed_binary_path_replaces_only_its_own_entry() {
    let dir = folder("moved");
    let path = dir.join("hooks.json");
    std::fs::write(&path, USERS).unwrap();
    apply(&path, &bin(), Os::Mac, Want::Install, 1).unwrap();
    let elsewhere = PathBuf::from("/opt/notchling/notchling");
    let again = apply(&path, &elsewhere, Os::Mac, Want::Install, 2).unwrap();
    assert!(again.wrote);
    let value = json_of(&path);
    assert_eq!(value["notchling"], desired(&elsewhere, Os::Mac));
    assert!(value.get("lint").is_some());
}

#[test]
fn a_backup_is_made_before_a_write_and_holds_the_old_text() {
    let dir = folder("backup");
    let path = dir.join("hooks.json");
    std::fs::write(&path, USERS).unwrap();
    let done = apply(&path, &bin(), Os::Mac, Want::Install, 7).unwrap();
    let backup = done.backup.expect("a backup of the old file");
    assert_eq!(std::fs::read_to_string(backup).unwrap(), USERS);
}

#[test]
fn a_missing_file_is_created_and_a_missing_folder_is_not() {
    let dir = folder("missing");
    let path = dir.join("hooks.json");
    let done = apply(&path, &bin(), Os::Mac, Want::Install, 1).unwrap();
    assert!(done.wrote);
    assert!(done.backup.is_none());
    assert_eq!(json_of(&path)["notchling"], desired(&bin(), Os::Mac));
    let none = dir.join("nofolder/hooks.json");
    assert!(apply(&none, &bin(), Os::Mac, Want::Install, 1).is_err());
    assert!(!none.exists());
    let other = dir.join("other.json");
    let done = apply(&other, &bin(), Os::Mac, Want::Remove, 1).unwrap();
    assert!(!done.wrote);
    assert!(!other.exists());
}

#[test]
fn switching_away_removes_only_its_own_entries_even_when_mixed_into_another_hook() {
    let mixed = json!({
        "mine": {
            "PreToolUse": [
                {"matcher": "run_command", "hooks": [
                    {"command": "./safety.sh"},
                    {"type": "command", "command": format!("'{BIN}' agy-hook pre-tool"), "timeout": 120}
                ]}
            ],
            "PreInvocation": [
                {"command": format!("'{BIN}' agy-hook pre-invocation")}
            ]
        },
        "notchling": desired(&bin(), Os::Mac),
        "keep": {"Stop": [{"command": "./s.sh"}]}
    })
    .to_string();
    let out = change(&mixed, &bin(), Os::Mac, Want::Remove)
        .unwrap()
        .unwrap();
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        value,
        json!({
            "mine": {"PreToolUse": [{"matcher": "run_command", "hooks": [{"command": "./safety.sh"}]}]},
            "keep": {"Stop": [{"command": "./s.sh"}]}
        })
    );
}

#[test]
fn removing_with_nothing_of_ours_there_writes_nothing() {
    assert_eq!(change(USERS, &bin(), Os::Mac, Want::Remove).unwrap(), None);
    assert_eq!(change("", &bin(), Os::Mac, Want::Remove).unwrap(), None);
    assert_eq!(change("{}", &bin(), Os::Mac, Want::Remove).unwrap(), None);
}

#[test]
fn a_file_that_is_not_valid_is_left_alone() {
    let dir = folder("bad");
    let path = dir.join("hooks.json");
    for bad in ["{ not json", "[]", "\"x\""] {
        std::fs::write(&path, bad).unwrap();
        for want in [Want::Install, Want::Remove] {
            let result = apply(&path, &bin(), Os::Mac, want, 1);
            assert!(result.is_err(), "{bad} {want:?}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
        }
        assert!(backups(&dir).is_empty());
    }
}

#[test]
fn a_hook_named_notchling_that_is_not_ours_is_never_taken_over() {
    let theirs = "{\"notchling\": {\"PreToolUse\": [{\"matcher\": \"*\", \"hooks\": [{\"command\": \"./mine.sh\"}]}]}}";
    assert!(change(theirs, &bin(), Os::Mac, Want::Install).is_err());
    assert_eq!(change(theirs, &bin(), Os::Mac, Want::Remove).unwrap(), None);
}

#[test]
fn found_tells_installed_from_absent_missing_and_unreadable() {
    let dir = folder("found");
    let path = dir.join("hooks.json");
    assert_eq!(found(&path), Found::Missing);
    std::fs::write(&path, USERS).unwrap();
    assert_eq!(found(&path), Found::Absent);
    apply(&path, &bin(), Os::Mac, Want::Install, 1).unwrap();
    assert_eq!(found(&path), Found::Installed);
    std::fs::write(&path, "{ junk").unwrap();
    assert_eq!(found(&path), Found::Unreadable);
}
