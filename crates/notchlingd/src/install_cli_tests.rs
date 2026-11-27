use super::*;

fn raw(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn install(list: &[&str]) -> Result<Value, String> {
    let scanned = scan(&raw(list), INSTALL_USAGE, &["--part"])?;
    install_body(&scanned)
}

fn cost(list: &[&str], reset: bool) -> Result<Value, String> {
    let scanned = scan(
        &raw(list),
        COST_USAGE,
        &["--preset", "--tool", "--pick", "--habit", "--consent"],
    )?;
    cost_body(&scanned, reset)
}

#[test]
fn an_install_request_names_its_parts_as_the_daemon_knows_them() {
    assert_eq!(install(&["request"]).unwrap(), json!({"consent": {}}));
    assert_eq!(
        install(&[
            "request",
            "--part",
            "hooks",
            "--part",
            "skill:claude:notchling-ask"
        ])
        .unwrap()["parts"],
        json!(["prompt_hooks", "page_hooks", "guard_hooks", "notchling-ask"])
    );
    assert_eq!(
        install(&[
            "request",
            "--part",
            "sidecar",
            "--reason",
            "why",
            "--dry-run"
        ])
        .unwrap(),
        json!({"consent": {"reason": "why"}, "parts": ["sidecar"], "dryRun": true})
    );
    assert!(install(&[]).is_err());
    assert!(install(&["apply"]).is_err());
    assert!(install(&["request", "--part"]).is_err());
}

#[test]
fn a_profile_change_becomes_one_profile_request() {
    let body = cost(
        &[
            "set",
            "--preset",
            "saver",
            "--tool",
            "codex=off",
            "--pick",
            "code.claude=sonnet high",
            "--habit",
            "maxTurns=20",
            "--habit",
            "readLines=default",
        ],
        true,
    )
    .unwrap();
    assert_eq!(body["action"], "profile");
    assert_eq!(
        body["args"],
        json!({
            "preset": "saver",
            "tools": {"codex": false},
            "picks": {"code.claude": "sonnet high"},
            "habits": {"maxTurns": 20, "readLines": null},
            "reset": true
        })
    );
}

#[test]
fn setup_and_undo_are_requests_and_bad_values_are_usage_errors() {
    let body = cost(&["setup", "--consent", "rules,pstack"], false).unwrap();
    assert_eq!(body["action"], "apply");
    assert_eq!(body["args"]["consent"], json!(["rules", "pstack"]));
    assert_eq!(cost(&["setup"], false).unwrap()["args"], json!({}));
    let body = cost(&["undo", "claude", "--key", "k"], false).unwrap();
    assert_eq!(body["action"], "undo");
    assert_eq!(body["args"], json!({"tool": "claude"}));
    assert_eq!(body["consent"], json!({"key": "k"}));
    assert!(cost(&["undo"], false).is_err());
    assert!(cost(&["wipe"], false).is_err());
    assert!(cost(&["set", "--tool", "codex=maybe"], false).is_err());
    assert!(cost(&["set", "--habit", "maxTurns=many"], false).is_err());
    assert!(cost(&["set", "--pick", "nope"], false).is_err());
}

#[test]
fn the_done_line_names_what_was_written_and_what_went_stale() {
    let request = json!({"result": {"action": "apply", "done": true, "parts": [
        {"id": "a", "state": "written"}, {"id": "b", "state": "stale"}]}});
    let line = done_line(&request);
    assert!(line.contains("Done: apply"));
    assert!(line.contains("written: a"));
    assert!(line.contains("stale: b"));
    assert!(line.contains("Ask again"));
    assert!(!done_line(&json!({"result": {"action": "profile", "done": true}})).contains("stale"));
}

#[test]
fn an_install_plan_is_a_read_that_names_what_to_plan() {
    let plan = |list: &[&str]| {
        let scanned = scan(&raw(list), INSTALL_USAGE, &["--part", "--what"])?;
        plan_body(&scanned)
    };
    assert_eq!(plan(&["plan"]).unwrap(), json!({}));
    assert_eq!(
        plan(&["plan", "--what", "prompt_hooks,notchling-ask"]).unwrap(),
        json!({"what": ["prompt_hooks", "notchling-ask"]})
    );
    assert!(plan(&["plan", "extra"]).is_err());
    assert!(plan(&["request"]).is_err());
}
