use super::status;
use crate::sessions::testutil::TempDir;

#[test]
fn the_flag_edit_changes_only_the_flags_text() {
    let root = TempDir::new("chrome-text");
    let original = "{\n  \"z\": 1.50,\n  \"n\": 1e3,\n  \"nest\": {\"respawnFlags\": [\"keep\"]},\n  \"respawnFlags\" : [ \"--name\" , \"a\\\"b\" ],\n  \"inFlight\": {\"tasks\": 0}\n}\n";
    let file = root.write("state.json", original);
    let flags: Vec<String> = ["--name", "a\"b", "--chrome"]
        .iter()
        .map(|f| f.to_string())
        .collect();
    status::set_flags(&file, &flags).unwrap();
    let after = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        after,
        original.replace(
            "[ \"--name\" , \"a\\\"b\" ]",
            "[\"--name\",\"a\\\"b\",\"--chrome\"]"
        )
    );
    assert_eq!(status::read_flags(&file).unwrap(), flags);
}

#[test]
fn a_linked_state_file_is_left_alone_and_the_copy_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let root = TempDir::new("chrome-link");
    let real = root.write("real.json", r#"{"respawnFlags":["--no-chrome"]}"#);
    let link = root.path("state.json");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let flags = vec!["--chrome".to_string()];
    assert!(status::set_flags(&link, &flags).is_err());
    assert_eq!(status::read_flags(&real).unwrap(), ["--no-chrome"]);
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
    status::set_flags(&real, &flags).unwrap();
    let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
    assert!(!root.path("real.json.notchling-tmp").exists());
}

#[test]
fn a_pending_change_is_written_read_and_cleared() {
    let root = TempDir::new("chrome-pending");
    let state = root.write("state.json", r#"{"respawnFlags":[]}"#);
    assert_eq!(status::read_pending(&state), None);
    let pending = status::Pending {
        at_ms: 5,
        on: true,
        before: vec!["--no-chrome".to_string()],
        wanted: vec!["--chrome".to_string()],
    };
    status::write_pending(&state, &pending).unwrap();
    assert_eq!(status::read_pending(&state), Some(pending));
    status::clear_pending(&state);
    assert_eq!(status::read_pending(&state), None);
}

#[test]
fn the_permission_mode_is_read_from_the_transcript_and_kept_in_the_flags() {
    let root = TempDir::new("chrome-mode");
    let transcript = root.write(
        "t.jsonl",
        concat!(
            r#"{"type":"permission-mode","permissionMode":"plan","sessionId":"s"}"#,
            "\n",
            r#"{"type":"user","message":"hi"}"#,
            "\n",
            r#"{"type":"permission-mode","permissionMode":"acceptEdits","sessionId":"s"}"#,
            "\n",
            r#"{"type":"user","message":"again"}"#,
            "\n"
        ),
    );
    assert_eq!(
        status::permission_mode(&transcript).as_deref(),
        Some("acceptEdits")
    );
    let odd = root.write(
        "u.jsonl",
        "{\"type\":\"permission-mode\",\"permissionMode\":\"bypassPermissions\"}\n",
    );
    assert_eq!(status::permission_mode(&odd), None);
    let flags: Vec<String> = ["--permission-mode", "auto", "--model", "m", "--chrome"]
        .iter()
        .map(|f| f.to_string())
        .collect();
    assert_eq!(
        status::with_mode(&flags, "plan"),
        ["--model", "m", "--chrome", "--permission-mode", "plan"]
    );
}

#[test]
fn only_the_last_permission_mode_counts_and_an_unknown_one_carries_nothing() {
    let root = TempDir::new("chrome-mode-last");
    let entry =
        |mode: &str| format!("{{\"type\":\"permission-mode\",\"permissionMode\":\"{mode}\"}}\n");
    let stale = root.write(
        "a.jsonl",
        &format!("{}{}", entry("auto"), entry("bypassPermissions")),
    );
    assert_eq!(status::permission_mode(&stale), None);
    let newer = root.write("b.jsonl", &format!("{}{}", entry("dontAsk"), entry("plan")));
    assert_eq!(status::permission_mode(&newer).as_deref(), Some("plan"));
    let blank = root.write(
        "c.jsonl",
        &format!("{}{{\"type\":\"permission-mode\"}}\n", entry("auto")),
    );
    assert_eq!(status::permission_mode(&blank), None);
    let none = root.write("d.jsonl", "{\"type\":\"user\"}\n");
    assert_eq!(status::permission_mode(&none), None);
}

#[test]
fn the_edited_flags_text_reads_back_as_exactly_the_flags_given() {
    let root = TempDir::new("chrome-roundtrip");
    let file = root.write("state.json", r#"{"respawnFlags":["--x"],"k":1}"#);
    let flags: Vec<String> = ["--name", "é \"q\" \\ \n", "--chrome"]
        .iter()
        .map(|f| f.to_string())
        .collect();
    status::set_flags(&file, &flags).unwrap();
    assert_eq!(status::read_flags(&file).unwrap(), flags);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(status::with_flags_text(&text, &flags).is_some());
    assert!(status::with_flags_text(&text, &flags[..2]).is_some());
}
