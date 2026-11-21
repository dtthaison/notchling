use super::*;
use notchling_local_api::{ApiError, ErrorCode};

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn setup_is_a_dry_run_unless_apply_is_given() {
    let plan = parse(&words(&["setup"])).unwrap();
    assert_eq!(plan.job, Job::Setup { apply: false });
    assert_eq!(request(&plan).0, ops::COST_SETUP_PLAN);
    let apply = parse(&words(&[
        "setup",
        "--apply",
        "--consent",
        "pstack,rules",
        "--wait",
    ]))
    .unwrap();
    let (op, body) = request(&apply);
    assert_eq!(op, ops::COST_SETUP_APPLY);
    assert_eq!(body, json!({"consent": ["pstack", "rules"], "wait": true}));
}

#[test]
fn check_and_undo_take_their_words() {
    assert_eq!(
        request(&parse(&words(&["check", "--json"])).unwrap()).0,
        ops::COST_SETUP_CHECK
    );
    let undo = parse(&words(&["undo", "codex"])).unwrap();
    assert_eq!(
        request(&undo),
        (ops::COST_SETUP_UNDO, json!({"tool": "codex"}))
    );
}

#[test]
fn a_wrong_command_gets_the_usage() {
    for bad in [
        vec![],
        vec!["setup", "now"],
        vec!["undo"],
        vec!["undo", "gemini"],
        vec!["check", "--apply"],
        vec!["--consent"],
        vec!["setup", "--nope"],
    ] {
        let e = parse(&words(&bad)).unwrap_err();
        assert!(e.contains("Usage: notchling cost"), "{bad:?}: {e}");
    }
    assert_eq!(
        parse(&words(&["check", "--socket", "/tmp/s"]))
            .unwrap()
            .socket,
        Some(PathBuf::from("/tmp/s"))
    );
}

#[test]
fn a_plan_prints_each_change_its_diff_and_says_nothing_was_written() {
    let args = parse(&words(&["setup"])).unwrap();
    let reply = json!({"plan": {
        "actions": [
            {"kind": "create", "tool": "claude", "path": "/h/.claude/rules/notchling-cost.md", "diff": "+hello\n", "keys": []},
            {"kind": "unchanged", "tool": "claude", "path": "/h/x", "diff": "", "keys": []},
            {"kind": "offer", "tool": "claude", "path": "/h/.claude/agents/orchestrator.md", "needs": "orchestrator", "diff": "", "keys": [], "reason": "asks first"},
            {"kind": "update", "tool": "claude", "path": "/h/.claude/settings.json", "diff": "", "keys": [{"key": "bashOutputMaxChars", "before": null, "after": "12000"}]}
        ],
        "report": [{"line": 3, "text": "- a line", "fate": "kept"}]
    }});
    let text = shown(&args, &reply);
    assert!(text.contains("create    claude  /h/.claude/rules/notchling-cost.md"));
    assert!(text.contains("+hello"));
    assert!(!text.contains("/h/x"));
    assert!(text.contains("needs --consent orchestrator"));
    assert!(text.contains("bashOutputMaxChars: (unset) -> 12000"));
    assert!(text.contains("line   3  kept"));
    assert!(text.ends_with("This was a dry run. Nothing was written."));
    assert!(shown(&parse(&words(&["setup", "--json"])).unwrap(), &reply).contains("\"actions\""));
}

#[test]
fn an_empty_plan_says_so_and_check_and_undo_list_their_rows() {
    let empty = json!({"plan": {"actions": [], "report": []}});
    assert!(shown(&parse(&words(&["setup"])).unwrap(), &empty).starts_with("Nothing to do."));
    let rows = json!({"rows": [{"state": "edited", "tool": "claude", "path": "/p"}]});
    assert!(shown(&parse(&words(&["check"])).unwrap(), &rows).starts_with("edited"));
    assert!(
        shown(&parse(&words(&["check"])).unwrap(), &json!({"rows": []}))
            .contains("Nothing recorded")
    );
    let undone = json!({"on": true, "results": [{"outcome": "conflict", "path": "/p", "detail": "changed since"}]});
    let text = shown(&parse(&words(&["undo", "claude"])).unwrap(), &undone);
    assert!(text.contains("conflict") && text.contains("(changed since)"));
    let off = json!({"on": false, "message": "install is off"});
    assert!(shown(&parse(&words(&["undo", "claude"])).unwrap(), &off)
        .contains("Not undone: install is off"));
}

#[test]
fn applying_as_a_non_app_says_who_applies() {
    let api = |code, text: &str| ClientError::Api(ApiError::new(code, text));
    let applying = failure_of(
        api(ErrorCode::Forbidden, "only the Notchling app may do this"),
        ops::COST_SETUP_APPLY,
        true,
    );
    assert!(applying.line().contains("Settings, Cost Profile"));
    assert_eq!(applying.exit, 4);
    let reading = failure_of(
        api(ErrorCode::Forbidden, "only the Notchling app may do this"),
        ops::COST_SETUP_PLAN,
        false,
    );
    assert_eq!(
        reading.line(),
        "only the Notchling app may do this (refused)"
    );
    assert_eq!(failure_of(ClientError::Timeout, ops::COST_SETUP_PLAN, false).exit, 3);
    assert_eq!(failure_of(api(ErrorCode::UnknownOp, "x"), ops::COST_SETUP_PLAN, false).reason, "old_daemon");
}
