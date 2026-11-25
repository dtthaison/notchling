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

#[test]
fn a_state_file_with_stray_closing_braces_still_gives_its_flags() {
    let root = TempDir::new("chrome-stray-flags");
    let body = r#"{"respawnFlags":["--name","demo","--no-chrome"],"inFlight":{"tasks":0}}"#;
    let one = root.write("one.json", &format!("{body}}}"));
    let many = root.write("many.json", &format!("{body}}}\n}}]\n"));
    for file in [&one, &many] {
        assert_eq!(
            status::read_flags(file).unwrap(),
            ["--name", "demo", "--no-chrome"]
        );
    }
    let junk = root.write("junk.json", &format!("{body} x"));
    assert_eq!(status::read_flags(&junk), None);
    let second = root.write("second.json", &format!("{body}{{}}"));
    assert_eq!(status::read_flags(&second), None);
    let list = root.write("list.json", "[1]}");
    assert_eq!(status::read_flags(&list), None);
}

#[test]
fn work_in_flight_ignores_stray_closing_braces_after_the_state_file() {
    let root = TempDir::new("chrome-stray-work");
    let busy = root.write(
        "busy.json",
        r#"{"state":"busy","inFlight":{"tasks":2,"queued":1}}}"#,
    );
    assert_eq!(status::tasks_in_flight(&busy), Some(3));
    let idle = root.write(
        "idle.json",
        "{\"inFlight\":{\"tasks\":0,\"queued\":0}}\n}\n}",
    );
    assert_eq!(status::tasks_in_flight(&idle), Some(0));
    let none = root.write("none.json", r#"{"state":"done"}}"#);
    assert_eq!(status::tasks_in_flight(&none), None);
}

#[test]
fn a_pending_change_file_with_stray_closing_braces_is_still_read() {
    let root = TempDir::new("chrome-stray-pending");
    let state = root.write("state.json", r#"{"respawnFlags":[]}"#);
    let body = r#"{"at_ms":7,"on":true,"before":["--no-chrome"],"wanted":["--chrome"]}"#;
    root.write(status::PENDING_FILE, &format!("{body}}}"));
    assert_eq!(
        status::read_pending(&state),
        Some(status::Pending {
            at_ms: 7,
            on: true,
            before: vec!["--no-chrome".to_string()],
            wanted: vec!["--chrome".to_string()],
        })
    );
    root.write(status::PENDING_FILE, &format!("{body}}} x"));
    assert_eq!(status::read_pending(&state), None);
}

#[test]
fn the_flags_of_a_state_file_with_stray_closing_braces_can_be_edited() {
    let root = TempDir::new("chrome-stray-edit");
    let file = root.write(
        "state.json",
        "{\"respawnFlags\":[\"--name\",\"demo\"],\"keep\":1}}\n",
    );
    status::rewrite_flags(&file, true).unwrap();
    assert_eq!(
        status::read_flags(&file).unwrap(),
        ["--name", "demo", "--chrome"]
    );
    assert_eq!(status::tasks_in_flight(&file), None);
    let text = std::fs::read_to_string(&file).unwrap();
    let strict: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        strict["respawnFlags"],
        serde_json::json!(["--name", "demo", "--chrome"])
    );
    assert_eq!(strict["keep"], 1);
    assert_eq!(
        text,
        "{\"respawnFlags\":[\"--name\",\"demo\",\"--chrome\"],\"keep\":1}\n"
    );
}

#[test]
fn an_edit_drops_every_stray_tail_and_keeps_the_rest_byte_for_byte() {
    let root = TempDir::new("chrome-stray-tails");
    let pretty =
        "{\n  \"z\": 1.50,\n  \"respawnFlags\" : [ \"--name\" , \"a\" ],\n  \"s\": \"}\\\"]\"\n}";
    let cases = [
        (format!("{pretty}}}"), pretty.to_string()),
        (format!("{pretty}}}\n"), format!("{pretty}\n")),
        (format!("{pretty}\n}}]\n"), format!("{pretty}\n")),
        (format!("{pretty}\n"), format!("{pretty}\n")),
        (format!("{pretty}\n\n"), format!("{pretty}\n\n")),
        (pretty.to_string(), pretty.to_string()),
    ];
    for (before, after) in cases {
        let file = root.write("state.json", &before);
        status::rewrite_flags(&file, true).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        let expected = after.replace("[ \"--name\" , \"a\" ]", "[\"--name\",\"a\",\"--chrome\"]");
        assert_eq!(text, expected, "{before:?}");
        let strict: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(strict["z"], 1.5);
        assert_eq!(strict["s"], "}\"]");
        assert_eq!(
            status::read_flags(&file).unwrap(),
            ["--name", "a", "--chrome"]
        );
    }
}

#[test]
fn the_edited_text_of_a_file_with_a_stray_tail_is_strictly_valid_json() {
    let flags = vec!["--name".to_string(), "x".to_string()];
    for tail in ["}", "}\n", "]}\n}", " }  \n"] {
        let text = format!("{{\"respawnFlags\":[\"--a\"],\"k\":{{\"n\":[1,2]}}}}{tail}");
        let out = status::with_flags_text(&text, &flags).unwrap();
        let strict: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(strict["respawnFlags"], serde_json::json!(["--name", "x"]));
        assert_eq!(strict["k"]["n"], serde_json::json!([1, 2]));
    }
}

#[test]
fn the_default_and_the_mode_are_read_past_stray_closing_braces() {
    let root = TempDir::new("chrome-stray-default");
    let claude = root.write(".claude.json", r#"{"claudeInChromeDefaultEnabled":true}}"#);
    assert_eq!(status::default_enabled(&claude), Some(true));
    let transcript = root.write(
        "t.jsonl",
        "{\"type\":\"permission-mode\",\"permissionMode\":\"plan\"}}\n",
    );
    assert_eq!(
        status::permission_mode(&transcript).as_deref(),
        Some("plan")
    );
}
