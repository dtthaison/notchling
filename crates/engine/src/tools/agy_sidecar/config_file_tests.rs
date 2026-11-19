use super::config_file::{check_enable, disable, enable, switch, EnableError, Switch};
use super::SIDECAR_ID;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const CONFIG: &str = "{\n  \"plugins\": {\n    \"superpowers\": {\"installedFrom\": {\"version\": \"7.0.2\"}}\n  },\n  \"sidecars\": {\n    \"swarm/swarm_board\": {\n      \"projectId\": \"8bd34639\",\n      \"schemaVersion\": 1\n    }\n  },\n  \"userSettings\": {\"autoExecutionPolicy\": \"EAGER\", \"big\": 12345678901234567890, \"ratio\": 1.50}\n}\n";

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-config-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn put(dir: &Path, text: &str) -> PathBuf {
    let path = dir.join("config.json");
    std::fs::write(&path, text).unwrap();
    path
}

fn backups(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().contains(".notchling-backup"))
        .collect()
}

#[test]
fn it_writes_nothing_without_a_confirmation() {
    let dir = folder("confirm");
    let path = put(&dir, CONFIG);
    assert!(matches!(
        enable(&path, SIDECAR_ID, false, 1),
        Err(EnableError::NotConfirmed)
    ));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), CONFIG);
    assert!(backups(&dir).is_empty());
}

#[test]
fn it_sets_only_the_one_key_and_every_other_byte_stays() {
    let dir = folder("one");
    let path = put(&dir, CONFIG);
    let done = enable(&path, SIDECAR_ID, true, 5).unwrap();
    assert!(done.wrote);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"big\": 12345678901234567890"));
    assert!(text.contains("\"ratio\": 1.50"));
    assert!(text.contains("\"superpowers\": {\"installedFrom\": {\"version\": \"7.0.2\"}}"));
    let mut before: Value = serde_json::from_str(CONFIG).unwrap();
    let after: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(after["sidecars"][SIDECAR_ID], json!({"enabled": true}));
    assert_eq!(
        after["sidecars"]["swarm/swarm_board"],
        before["sidecars"]["swarm/swarm_board"]
    );
    before["sidecars"][SIDECAR_ID] = json!({"enabled": true});
    assert_eq!(after, before);
    assert_eq!(switch(&path, SIDECAR_ID), Ok(Switch::On));
}

#[test]
fn it_backs_up_the_old_file_first() {
    let dir = folder("backup");
    let path = put(&dir, CONFIG);
    let done = enable(&path, SIDECAR_ID, true, 5).unwrap();
    let backup = done.backup.expect("a backup");
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), CONFIG);
    assert_eq!(backups(&dir).len(), 1);
}

#[test]
fn an_existing_entry_keeps_its_other_fields_and_is_only_switched_on() {
    let dir = folder("entry");
    let text = format!(
        "{{\"sidecars\": {{\"{SIDECAR_ID}\": {{\"enabled\": false, \"projectId\": \"p1\", \"schemaVersion\": 1}}}}}}"
    );
    let path = put(&dir, &text);
    assert_eq!(switch(&path, SIDECAR_ID), Ok(Switch::Off));
    enable(&path, SIDECAR_ID, true, 1).unwrap();
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        after["sidecars"][SIDECAR_ID],
        json!({"enabled": true, "projectId": "p1", "schemaVersion": 1})
    );
}

#[test]
fn turning_it_on_twice_writes_once() {
    let dir = folder("twice");
    let path = put(&dir, CONFIG);
    assert!(enable(&path, SIDECAR_ID, true, 1).unwrap().wrote);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!enable(&path, SIDECAR_ID, true, 2).unwrap().wrote);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert_eq!(backups(&dir).len(), 1);
}

#[test]
fn a_config_without_a_sidecars_key_gets_one() {
    let dir = folder("nokey");
    let path = put(&dir, "{\n  \"plugins\": {}\n}\n");
    enable(&path, SIDECAR_ID, true, 1).unwrap();
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(after["sidecars"][SIDECAR_ID]["enabled"], true);
    assert_eq!(after["plugins"], json!({}));
}

#[test]
fn a_missing_empty_or_malformed_config_is_never_written() {
    let dir = folder("bad");
    let path = dir.join("config.json");
    assert!(matches!(
        enable(&path, SIDECAR_ID, true, 1),
        Err(EnableError::NoConfig)
    ));
    assert!(!path.exists());
    for bad in [
        "",
        "{ nope",
        "[]",
        "{\"sidecars\": []}",
        "{\"sidecars\": {\"notchling/sidecar\": 3}}",
    ] {
        put(&dir, bad);
        assert!(
            matches!(
                enable(&path, SIDECAR_ID, true, 1),
                Err(EnableError::File(_))
            ),
            "{bad}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
    }
    assert!(backups(&dir).is_empty());
}

#[test]
fn the_switch_reads_unset_when_there_is_nothing_and_errors_on_junk() {
    let dir = folder("read");
    let path = dir.join("config.json");
    assert_eq!(switch(&path, SIDECAR_ID), Ok(Switch::Unset));
    put(&dir, CONFIG);
    assert_eq!(switch(&path, SIDECAR_ID), Ok(Switch::Unset));
    put(&dir, "{ junk");
    assert!(switch(&path, SIDECAR_ID).is_err());
}

#[test]
fn on_then_off_gives_back_the_file_it_was_given_and_the_switch_reads_unset() {
    for original in [
        CONFIG.to_string(),
        "{\n  \"plugins\": {}\n}\n".to_string(),
        "{\"a\":1,\"big\":12345678901234567890}".to_string(),
    ] {
        let dir = folder("roundtrip");
        let path = put(&dir, &original);
        enable(&path, SIDECAR_ID, true, 1).unwrap();
        assert_eq!(switch(&path, SIDECAR_ID), Ok(Switch::On));
        let off = disable(&path, SIDECAR_ID, 2).unwrap();
        assert!(off.wrote);
        assert!(off.backup.is_some());
        assert_eq!(switch(&path, SIDECAR_ID), Ok(Switch::Unset));
        let after = std::fs::read_to_string(&path).unwrap();
        let a: Value = serde_json::from_str(&after).unwrap();
        let b: Value = serde_json::from_str(&original).unwrap();
        assert_eq!(a, b);
        if !original.contains("swarm") {
            assert_eq!(after, original);
        }
    }
}

#[test]
fn turning_off_removes_only_our_key_and_keeps_the_entry_fields_and_other_sidecars() {
    let dir = folder("offkeeps");
    let text = format!(
        "{{\"before\": 1, \"sidecars\": {{\"swarm/swarm_board\": {{\"projectId\": \"x\"}}, \"{SIDECAR_ID}\": {{\"enabled\": true, \"projectId\": \"p1\"}}}}, \"after\": 2.50}}"
    );
    let path = put(&dir, &text);
    disable(&path, SIDECAR_ID, 3).unwrap();
    let now = std::fs::read_to_string(&path).unwrap();
    assert!(now.starts_with("{\"before\": 1, \"sidecars\": "));
    assert!(now.ends_with(", \"after\": 2.50}"));
    let after: Value = serde_json::from_str(&now).unwrap();
    assert_eq!(after["sidecars"][SIDECAR_ID], json!({"projectId": "p1"}));
    assert_eq!(
        after["sidecars"]["swarm/swarm_board"],
        json!({"projectId": "x"})
    );
    assert_eq!(switch(&path, SIDECAR_ID), Ok(Switch::Unset));
}

#[test]
fn turning_off_changes_nothing_when_we_never_turned_it_on_or_the_file_is_not_there() {
    let dir = folder("offnothing");
    let path = dir.join("config.json");
    assert!(!disable(&path, SIDECAR_ID, 1).unwrap().wrote);
    put(&dir, CONFIG);
    assert!(!disable(&path, SIDECAR_ID, 1).unwrap().wrote);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), CONFIG);
    let own_false = format!("{{\"sidecars\": {{\"{SIDECAR_ID}\": {{\"enabled\": false}}}}}}");
    put(&dir, &own_false);
    assert!(!disable(&path, SIDECAR_ID, 1).unwrap().wrote);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), own_false);
    assert!(backups(&dir).is_empty());
}

#[test]
fn the_check_before_turning_on_refuses_what_turning_on_would_refuse_and_writes_nothing() {
    let dir = folder("check");
    let path = dir.join("config.json");
    assert!(matches!(
        check_enable(&path, SIDECAR_ID),
        Err(EnableError::NoConfig)
    ));
    for bad in [
        "",
        "{ nope",
        "[]",
        "{\"sidecars\": []}",
        "{\"sidecars\": {\"notchling/sidecar\": 3}}",
    ] {
        put(&dir, bad);
        assert!(
            matches!(check_enable(&path, SIDECAR_ID), Err(EnableError::File(_))),
            "{bad}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
    }
    put(&dir, CONFIG);
    assert!(check_enable(&path, SIDECAR_ID).is_ok());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), CONFIG);
    assert!(backups(&dir).is_empty());
}
