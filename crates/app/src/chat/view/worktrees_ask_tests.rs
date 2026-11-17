use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use serde_json::json;

use super::*;
use crate::chat::git::Why;
use crate::chat::view::repo::{folder_of, own_stat_request};

fn agent(cwd: &str, worktree: Option<&str>) -> Agent {
    Agent {
        id: "a".into(),
        name: "a".into(),
        project: "p".into(),
        cwd: cwd.into(),
        group: AgentGroup::Working,
        label: String::new(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job("a".into()),
        suggested_reply: None,
        transcript_path: None,
        host: String::new(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: worktree.map(str::to_owned),
    }
}

fn reply(branch: &str) -> Value {
    json!({
        "repo": true, "name": "r", "default": "origin/main", "integration": null,
        "fetched_at": null, "no_worktree": 3, "truncated": false,
        "worktrees": [
            {"path": "/r", "real": "/r", "main": true, "current": true, "branch": branch}
        ]
    })
}

#[test]
fn failed_call_keeps_last_rows() {
    let mut state = WorktreesState::default();
    assert!(state.apply("/r", Ok(reply("main"))));
    let kept = state.rows_for(Some("/r"));
    assert_eq!(kept.as_ref().map(|r| r.list.len()), Some(1));
    assert!(!state.apply("/r", Err("timed out".into())));
    let after = state.rows_for(Some("/r"));
    assert!(Arc::ptr_eq(
        kept.as_ref().expect("rows"),
        after.as_ref().expect("rows")
    ));
    assert!(state.rows_for(Some("/elsewhere")).is_none());
}

#[test]
fn an_unchanged_reply_makes_no_new_model() {
    let mut state = WorktreesState::default();
    assert!(state.apply("/r", Ok(reply("main"))));
    let first = state.rows_for(Some("/r")).expect("rows");
    assert!(!state.apply("/r", Ok(reply("main"))));
    let second = state.rows_for(Some("/r")).expect("rows");
    assert!(Arc::ptr_eq(&first, &second));
    assert!(state.apply("/r", Ok(reply("dev"))));
}

#[test]
fn no_worktrees_call_before_status_says_repo() {
    let mut state = WorktreesState::default();
    assert!(!state.due(false, Why::Opened, Some("/r"), true, 1_000));
    assert!(state.due(true, Why::Opened, Some("/r"), true, 1_000));
    assert!(
        !state.due(true, Why::Opened, Some("/r"), true, 1_100),
        "one in flight"
    );
}

#[test]
fn stat_asks_pick_nearest_unless_base_chosen() {
    let nearest = own_stat_request("/r", None);
    assert_eq!(nearest.args, json!({"cwd": "/r", "pick": "nearest"}));
    let chosen = own_stat_request("/r", Some("origin/main"));
    assert_eq!(chosen.args, json!({"cwd": "/r", "base": "origin/main"}));
}

#[test]
fn selected_folder_follows_worktree_path() {
    assert_eq!(
        folder_of(&agent("/r", Some("/r/lane"))).as_deref(),
        Some("/r/lane")
    );
    assert_eq!(folder_of(&agent("/r", None)).as_deref(), Some("/r"));
    assert_eq!(folder_of(&agent("", None)), None);
}
