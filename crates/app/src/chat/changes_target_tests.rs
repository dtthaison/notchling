use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::chat::branch::{BranchModel, ChangedFile};

#[test]
fn own_target_asks_pick_nearest() {
    let r = stat_request("/r", &Target::Own, None);
    assert_eq!(r.op, "git.branch_diff_stat");
    assert_eq!(r.args, json!({"cwd": "/r", "pick": "nearest"}));
}

#[test]
fn explicit_base_drops_pick() {
    let r = stat_request("/r", &Target::Own, Some("origin/main"));
    assert_eq!(r.args, json!({"cwd": "/r", "base": "origin/main"}));
    let w = stat_request("/r", &Target::Worktree("/r/lane".into()), Some("int"));
    assert!(w.args.get("pick").is_none());
}

#[test]
fn worktree_target_args() {
    let t = Target::Worktree("/r/.claude/worktrees/lane".into());
    let stat = stat_request("/r", &t, None);
    assert_eq!(
        stat.args,
        json!({"cwd": "/r", "worktree": "/r/.claude/worktrees/lane", "pick": "nearest"})
    );
    let file = file_request("/r", &t, "int", "a.rs", Some("old.rs"));
    assert_eq!(file.op, "git.branch_diff_file");
    assert_eq!(
        file.args,
        json!({
            "cwd": "/r", "worktree": "/r/.claude/worktrees/lane",
            "base": "int", "path": "a.rs", "from": "old.rs"
        })
    );
    let own = file_request("/r", &Target::Own, "int", "a.rs", None);
    assert_eq!(
        own.args,
        json!({"cwd": "/r", "base": "int", "path": "a.rs"})
    );
    let branches = branches_request("/r", &t);
    assert_eq!(branches.op, "git.branches");
    assert_eq!(
        branches.args,
        json!({"cwd": "/r", "worktree": "/r/.claude/worktrees/lane"})
    );
    assert_eq!(
        branches_request("/r", &Target::Own).args,
        json!({"cwd": "/r"})
    );
}

#[test]
fn ref_target_args() {
    let t = Target::Ref("agent/ask".into());
    let stat = stat_request("/r", &t, Some("origin/main"));
    assert_eq!(
        stat.args,
        json!({"cwd": "/r", "ref": "agent/ask", "base": "origin/main"})
    );
    let nearest = stat_request("/r", &t, None);
    assert_eq!(
        nearest.args,
        json!({"cwd": "/r", "ref": "agent/ask", "pick": "nearest"})
    );
    let file = file_request("/r", &t, "origin/main", "b.rs", None);
    assert_eq!(
        file.args,
        json!({"cwd": "/r", "ref": "agent/ask", "base": "origin/main", "path": "b.rs"})
    );
}

#[test]
fn ref_branches_ask_in_cwd() {
    let r = branches_request("/r", &Target::Ref("agent/ask".into()));
    assert_eq!(r.args, json!({"cwd": "/r"}));
}

fn model(branch: &str, base: &str, added: u64) -> Arc<BranchModel> {
    Arc::new(BranchModel {
        branch: branch.into(),
        base: base.into(),
        merge_base: "abc".into(),
        files: vec![],
        added,
        removed: 1,
        file_count: 1,
        uncommitted_files: 0,
        truncated: false,
    })
}

fn reply(branch: &str, base: &str, added: u64) -> Value {
    json!({
        "repo": true, "branch": branch, "base": base, "merge_base": "abc",
        "added": added, "removed": 1, "file_count": 1, "files": []
    })
}

fn lane() -> Target {
    Target::Worktree("/r/lane".into())
}

#[test]
fn closing_other_target_keeps_own_numbers() {
    let own = Some(model("main", "origin/main", 10));
    let own_choice = Some("origin/main".to_owned());
    let mut other = Some(TargetState::new(lane()));
    let mut from_branches = false;
    let done = apply_stat(&mut other, &lane(), None, Ok(reply("lane", "int", 99)));
    assert_eq!(done, Applied::Changed);
    let shown = shown_branch(own.as_ref(), other.as_ref()).unwrap();
    assert_eq!((shown.branch.as_str(), shown.added), ("lane", 99));
    on_close(&mut other, &mut from_branches);
    let shown = shown_branch(own.as_ref(), other.as_ref()).unwrap();
    assert_eq!((shown.branch.as_str(), shown.added), ("main", 10));
    assert_eq!(own.as_ref().unwrap().base, "origin/main");
    assert_eq!(own_choice.as_deref(), Some("origin/main"));
}

#[test]
fn other_target_base_choice_is_its_own() {
    let own_choice = Some("origin/main".to_owned());
    let mut state = TargetState::new(lane());
    assert!(choose_base(&mut state, "feature".into()));
    assert!(!choose_base(&mut state, "feature".into()));
    assert_eq!(state.base_choice.as_deref(), Some("feature"));
    assert_eq!(own_choice.as_deref(), Some("origin/main"));
    let mut other = Some(state);
    assert_eq!(
        asking(other.as_ref()),
        Some((lane(), Some("feature".to_owned())))
    );
    assert_eq!(
        apply_stat(&mut other, &lane(), None, Ok(reply("lane", "int", 1))),
        Applied::Retry,
        "an answer to the old base choice is stale"
    );
    assert!(other.as_ref().unwrap().branch.is_none());
}

#[test]
fn refused_target_falls_back_to_own() {
    let own = Some(model("main", "origin/main", 10));
    let mut other = Some(TargetState::new(lane()));
    let done = apply_stat(
        &mut other,
        &lane(),
        None,
        Err("that worktree is gone".into()),
    );
    assert_eq!(done, Applied::Changed);
    assert!(shown_branch(own.as_ref(), other.as_ref()).is_none());
    assert_eq!(
        other.as_ref().unwrap().refused.as_deref(),
        Some("that worktree is gone")
    );
    assert_eq!(
        asking(other.as_ref()),
        None,
        "the next round asks the own folder only"
    );
    assert_eq!(shown_target(other.as_ref()), lane());
}

#[test]
fn a_rejected_base_clears_the_choice_and_asks_again() {
    let mut state = TargetState::new(lane());
    state.base_choice = Some("gone".into());
    let mut other = Some(state);
    let done = apply_stat(
        &mut other,
        &lane(),
        Some("gone"),
        Err("no such base".into()),
    );
    assert_eq!(done, Applied::Retry);
    let state = other.unwrap();
    assert_eq!(state.base_choice, None);
    assert_eq!(state.refused, None);
}

#[test]
fn an_answer_for_another_target_is_ignored() {
    let mut other = Some(TargetState::new(lane()));
    let done = apply_stat(
        &mut other,
        &Target::Ref("x".into()),
        None,
        Ok(reply("x", "main", 5)),
    );
    assert_eq!(done, Applied::Ignored);
    assert!(other.unwrap().branch.is_none());
    let mut none = None;
    assert_eq!(
        apply_stat(&mut none, &lane(), None, Ok(reply("x", "main", 5))),
        Applied::Ignored
    );
}

#[test]
fn an_unchanged_answer_is_not_a_change() {
    let mut other = Some(TargetState::new(lane()));
    apply_stat(&mut other, &lane(), None, Ok(reply("lane", "int", 1)));
    let again = apply_stat(&mut other, &lane(), None, Ok(reply("lane", "int", 1)));
    assert_eq!(again, Applied::Same);
}

#[test]
fn folder_change_clears_other_target() {
    let mut other = Some(TargetState::new(lane()));
    let mut from_branches = true;
    on_folder_change(&mut other, &mut from_branches);
    assert!(other.is_none() && !from_branches);
}

#[test]
fn file_request_carries_the_target() {
    let via = DiffVia {
        target: lane(),
        base: "int".into(),
        from: Some("old.rs".into()),
    };
    assert_eq!(
        via.request("/r", "a.rs").args,
        json!({"cwd": "/r", "worktree": "/r/lane", "base": "int", "path": "a.rs", "from": "old.rs"})
    );
    let own = DiffVia {
        target: Target::Own,
        base: "int".into(),
        from: None,
    };
    let r = own.request("/r", "a.rs");
    assert_eq!(r.op, "git.branch_diff_file");
    assert_eq!(r.args, json!({"cwd": "/r", "base": "int", "path": "a.rs"}));
}

fn file(path: &str, status: &str, uncommitted: bool) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        status: status.into(),
        from: None,
        added: Some(1),
        removed: Some(0),
        uncommitted,
    }
}

#[test]
fn offer_page_only_for_own_new_uncommitted_html() {
    let new = file("docs/a.html", "?", true);
    assert!(offers_page(&Target::Own, &new));
    assert!(offers_page(&Target::Own, &file("a.htm", "A", true)));
    assert!(!offers_page(&lane(), &new));
    assert!(!offers_page(&Target::Ref("x".into()), &new));
    assert!(!offers_page(&Target::Own, &file("a.html", "A", false)));
    assert!(!offers_page(&Target::Own, &file("a.html", "M", true)));
    assert!(!offers_page(&Target::Own, &file("a.html", "D", true)));
    assert!(!offers_page(&Target::Own, &file("a.rs", "?", true)));
}

#[test]
fn close_from_branches_returns_there() {
    let mut other = Some(TargetState::new(lane()));
    let mut from_branches = true;
    assert!(on_close(&mut other, &mut from_branches));
    assert!(other.is_none() && !from_branches);
    let mut again = false;
    assert!(!on_close(&mut other, &mut again));
}

#[test]
fn own_target_from_branches_returns_there() {
    let mut other: Option<TargetState> = None;
    let mut from_branches = true;
    assert!(on_close(&mut other, &mut from_branches));
    assert!(!from_branches);
}

#[test]
fn only_a_ref_target_is_committed_only() {
    assert!(is_ref(Some(&TargetState::new(Target::Ref("x".into())))));
    assert!(!is_ref(Some(&TargetState::new(lane()))));
    assert!(!is_ref(None));
}

#[test]
fn answer_without_the_new_target_asks_again() {
    let other = Some(TargetState::new(lane()));
    assert!(needs_ask_again(other.as_ref(), None, true, true));
}

#[test]
fn answer_for_an_older_target_asks_again() {
    let mut other = Some(TargetState::new(lane()));
    let done = apply_stat(
        &mut other,
        &Target::Ref("old".into()),
        None,
        Ok(reply("old", "main", 5)),
    );
    assert_eq!(done, Applied::Ignored);
    assert!(needs_ask_again(other.as_ref(), Some(&done), true, true));
    assert!(needs_ask_again(
        other.as_ref(),
        Some(&Applied::Retry),
        true,
        true
    ));
}

#[test]
fn answered_or_refused_target_does_not_ask_again() {
    let mut other = Some(TargetState::new(lane()));
    let done = apply_stat(&mut other, &lane(), None, Ok(reply("lane", "main", 5)));
    assert_eq!(done, Applied::Changed);
    assert!(!needs_ask_again(other.as_ref(), Some(&done), true, true));
    assert!(!needs_ask_again(
        other.as_ref(),
        Some(&Applied::Same),
        true,
        true
    ));
    let mut refused = Some(TargetState::new(lane()));
    apply_stat(&mut refused, &lane(), None, Err("no".into()));
    assert!(!needs_ask_again(refused.as_ref(), None, true, true));
    assert!(!needs_ask_again(None, None, true, true));
    let waiting = Some(TargetState::new(lane()));
    assert!(!needs_ask_again(waiting.as_ref(), None, false, true));
    assert!(!needs_ask_again(waiting.as_ref(), None, true, false));
}
