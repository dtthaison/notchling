use super::check::{CheckModel, Dot, Drift, Resolve};
use super::renders::{check, Shown, HOME};
use serde_json::json;

#[test]
fn each_state_has_a_dot() {
    assert_eq!(Drift::InStep.dot(), Dot::Done);
    assert_eq!(Drift::Edited.dot(), Dot::Needs);
    assert_eq!(Drift::Missing.dot(), Dot::Needs);
    assert_eq!(Drift::ChangedByTool.dot(), Dot::Working);
    assert_eq!(Drift::Outdated.dot(), Dot::Working);
    assert_eq!(Drift::Skipped.dot(), Dot::Muted);
}

#[test]
fn an_edited_file_can_be_kept_or_restored_and_a_missing_one_only_restored() {
    assert_eq!(
        Drift::Edited.resolutions(),
        [Resolve::Keep, Resolve::Restore]
    );
    assert_eq!(
        Drift::ChangedByTool.resolutions(),
        [Resolve::Keep, Resolve::Restore]
    );
    assert_eq!(Drift::Missing.resolutions(), [Resolve::Restore]);
    assert!(Drift::InStep.resolutions().is_empty());
    assert!(Drift::Skipped.resolutions().is_empty());
}

#[test]
fn the_choice_names_are_the_ops_words() {
    assert_eq!(Resolve::Keep.choice(), "keep");
    assert_eq!(Resolve::Restore.choice(), "restore");
}

#[test]
fn rows_are_read_with_their_tool_and_short_path() {
    let m = check(Shown::Check);
    assert_eq!(m.rows.len(), 7);
    assert_eq!(m.rows[0].path, "~/.claude/rules/notchling-cost.md");
    assert_eq!(m.rows[4].tool, 1);
    assert_eq!(m.rows[2].state, Drift::ChangedByTool);
    assert!(m.rows[2]
        .detail
        .as_deref()
        .unwrap()
        .contains("autoCompactWindow"));
}

#[test]
fn drift_counts_only_files_that_differ() {
    let m = check(Shown::Check);
    assert_eq!(m.drifted(), 4);
}

#[test]
fn nothing_can_be_resolved_while_the_install_switch_is_off() {
    let mut m = CheckModel::default();
    m.show(
        Ok(json!({"on": false, "drift": true, "rows": [
            {"id": "claude.rule", "tool": "claude", "path": "/x", "state": "edited", "detail": null},
        ]})),
        HOME,
    );
    assert!(!m.can_resolve());
    assert!(!m.start_resolve("claude.rule"));
    assert_eq!(m.busy, None);
}

#[test]
fn a_resolve_marks_its_row_busy_until_the_new_rows_arrive() {
    let mut m = check(Shown::Check);
    assert!(m.can_resolve());
    assert!(m.start_resolve("claude.agent.builder"));
    assert!(!m.start_resolve("claude.settings"));
    m.show(
        Ok(json!({"on": true, "drift": false, "rows": [
            {"id": "claude.agent.builder", "tool": "claude", "path": "/x", "state": "in_step", "detail": null},
        ]})),
        HOME,
    );
    assert_eq!(m.busy, None);
    assert_eq!(m.drifted(), 0);
}

#[test]
fn a_resolve_the_switch_refused_keeps_the_rows_and_shows_the_note() {
    let mut m = check(Shown::Check);
    m.show(
        Ok(json!({"on": false, "applied": false, "message": "install is off"})),
        HOME,
    );
    assert_eq!(m.rows.len(), 7);
    assert_eq!(m.message.as_deref(), Some("install is off"));
}

#[test]
fn a_failed_read_says_why() {
    let mut m = CheckModel::default();
    m.show(Err("the daemon isn't running".to_owned()), HOME);
    assert!(m.loaded);
    assert_eq!(m.message.as_deref(), Some("the daemon isn't running"));
}
