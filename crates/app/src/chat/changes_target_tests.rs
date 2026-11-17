use serde_json::json;

use super::*;

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
