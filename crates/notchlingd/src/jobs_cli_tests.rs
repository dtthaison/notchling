use super::*;

fn words(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_tail_prints_the_output_whether_free_or_after_a_card() {
    let free = json!({"free": true, "result": {"text": "line one\nline two\n", "next": 18}});
    assert_eq!(tail_text(&free), "line one\nline two");
    let asked = json!({"request": {"result": {"text": "from a card"}}});
    assert_eq!(tail_text(&asked), "from a card");
    let row = json!({"state": "applied", "result": {"text": "a row"}});
    assert_eq!(tail_text(&row), "a row");
    assert_eq!(tail_text(&json!({"result": {}})), "");
}

#[test]
fn a_dispatch_line_becomes_a_body_with_its_own_key() {
    let s = scan(
        &words(&[
            "build",
            "--tool",
            "shell",
            "--command",
            "make",
            "--secret",
            "a",
            "--secret",
            "b",
            "--key",
            "k1",
            "--dry-run",
        ]),
        DISPATCH_USAGE,
        &[
            "--tool",
            "--folder",
            "--model",
            "--effort",
            "--text",
            "--command",
            "--secret",
        ],
    )
    .unwrap();
    let body = dispatch_body(&s, "/here", "fresh").unwrap();
    assert_eq!(body["machine"], "build");
    assert_eq!(body["tool"], "shell");
    assert_eq!(body["cwd"], "/here");
    assert_eq!(body["key"], "k1");
    assert_eq!(body["secrets"], json!(["a", "b"]));
    assert_eq!(body["dryRun"], true);
    assert_eq!(body["consent"]["key"], "k1");
}

#[test]
fn without_a_key_each_run_gets_a_fresh_one() {
    let s = scan(
        &words(&["build", "--tool", "claude"]),
        DISPATCH_USAGE,
        &["--tool"],
    )
    .unwrap();
    assert_eq!(dispatch_body(&s, "/here", "fresh").unwrap()["key"], "fresh");
}

#[test]
fn a_dispatch_without_a_tool_or_machine_is_a_usage_error() {
    let s = scan(&words(&["build"]), DISPATCH_USAGE, &["--tool"]).unwrap();
    assert!(dispatch_body(&s, "/here", "f").is_err());
    let s = scan(&words(&["--tool", "shell"]), DISPATCH_USAGE, &["--tool"]).unwrap();
    assert!(dispatch_body(&s, "/here", "f").is_err());
    assert!(scan(&words(&["--nope"]), DISPATCH_USAGE, &[]).is_err());
}

#[test]
fn a_delegate_line_names_its_tool_and_folder() {
    let args = words(&["codex", "--folder", "/work/x", "--", "--folder", "/no"]);
    assert_eq!(
        delegate_tool_and_folder(&args, "/cwd"),
        ("codex".to_string(), "/work/x".to_string())
    );
    assert_eq!(
        delegate_tool_and_folder(&words(&["agy", "--", "hi"]), "/cwd"),
        ("agy".to_string(), "/cwd".to_string())
    );
}

#[test]
fn own_rows_print_in_full_and_others_by_state() {
    let own = json!({"id": "j-1", "own": true, "state": "running", "machine": "m", "tool": "shell", "folder": "/f"});
    assert_eq!(row_line(&own), "j-1  running  m  shell  /f");
    assert_eq!(
        row_line(&json!({"id": "j-2", "own": false, "state": "done"})),
        "j-2  done"
    );
}

#[test]
fn a_cost_request_first_call_waits_for_the_probes_the_daemon_runs() {
    let probes = notchling_engine::cost_profile::probe::WAIT * 3;
    assert!(first_call_wait(ops::COST_REQUEST) > probes);
    for op in [
        ops::MACHINES_REQUEST,
        ops::INSTALL_REQUEST,
        ops::CLUSTER_DISPATCH,
    ] {
        assert_eq!(first_call_wait(op), CALL_WAIT, "{op}");
    }
}
