use super::renders::{plan_reply, setup, versions_reply, Shown, HOME};
use super::setup::{diff_line, fate_text, Change, DiffLine, Outcome, Plan, SetupModel, Stage};
use serde_json::json;

fn plan(on: bool, ticked: bool) -> Plan {
    Plan::from_reply(&plan_reply(on, ticked), Some(&versions_reply()), HOME).unwrap()
}

#[test]
fn every_tool_gets_a_group_with_its_version() {
    let p = plan(true, false);
    assert_eq!(p.groups.len(), 3);
    assert_eq!(p.groups[0].version.as_deref(), Some("2.1.328"));
    assert_eq!(p.groups[1].version.as_deref(), Some("0.166.0"));
    assert!(p.groups[0].found && p.groups[1].found);
}

#[test]
fn a_missing_tool_is_skipped_and_lists_no_rows() {
    let p = plan(true, false);
    assert!(!p.groups[2].found);
    assert!(p.groups[2].actions.is_empty());
}

#[test]
fn rows_are_new_edit_replace_remove_then_offers() {
    let p = plan(true, false);
    let kinds: Vec<Change> = p.groups[0].actions.iter().map(|a| a.change).collect();
    assert_eq!(
        kinds,
        [
            Change::New,
            Change::New,
            Change::Edit,
            Change::Offer,
            Change::Offer,
            Change::Offer
        ]
    );
}

#[test]
fn a_ticked_replacement_of_a_whole_file_is_a_replace() {
    let p = plan(true, true);
    assert!(p.groups[0]
        .actions
        .iter()
        .any(|a| a.id == "claude.rules.replaced.model-choice.md" && a.change == Change::Replace));
}

#[test]
fn keys_and_blocks_are_edits_and_whole_files_are_new() {
    let p = plan(true, false);
    let settings = p.action("claude.settings").unwrap();
    assert_eq!(settings.change, Change::Edit);
    assert_eq!(settings.keys.len(), 2);
    assert_eq!(settings.keys[1].before, "");
    assert_eq!(p.action("codex.agents_md").unwrap().change, Change::New);
}

#[test]
fn rows_already_in_step_are_counted_not_listed() {
    let p = plan(true, false);
    assert_eq!(p.groups[0].in_step, 1);
    assert!(p.groups[0]
        .actions
        .iter()
        .all(|a| a.id != "claude.agent.reader"));
}

#[test]
fn paths_under_home_read_with_a_tilde() {
    let p = plan(true, false);
    assert_eq!(
        p.action("claude.rule").unwrap().path,
        "~/.claude/rules/notchling-cost.md"
    );
}

#[test]
fn offers_start_unticked_and_ticking_asks_for_their_consent() {
    let mut m = setup(Shown::Setup);
    let offer = m
        .plan
        .as_ref()
        .unwrap()
        .action("claude.orchestrator.new")
        .unwrap()
        .clone();
    assert!(!m.ticked(&offer));
    assert!(m.consent_list().is_empty());
    assert!(m.toggle_offer("claude.orchestrator.new"));
    assert_eq!(m.consent_list(), ["orchestrator_new"]);
    assert!(m.ticked(&offer));
    assert!(m.toggle_offer("claude.orchestrator.new"));
    assert!(m.consent_list().is_empty());
}

#[test]
fn a_row_that_needs_nothing_cannot_be_ticked() {
    let mut m = setup(Shown::Setup);
    assert!(!m.toggle_offer("claude.rule"));
    assert!(!m.toggle_offer("no.such.row"));
}

#[test]
fn a_diff_opens_and_closes_by_row() {
    let mut m = SetupModel::new(HOME);
    m.toggle_diff("claude.rule");
    assert!(m.is_open("claude.rule"));
    m.toggle_diff("claude.rule");
    assert!(!m.is_open("claude.rule"));
}

#[test]
fn apply_needs_the_switch_and_something_to_write() {
    assert!(setup(Shown::Setup).can_apply());
    assert!(!setup(Shown::SetupOff).can_apply());
    let mut m = setup(Shown::Setup);
    m.stage = Stage::Applying;
    assert!(!m.can_apply());
}

#[test]
fn diff_lines_are_told_apart() {
    assert_eq!(diff_line("+++ /a"), DiffLine::Header);
    assert_eq!(diff_line("--- /a"), DiffLine::Header);
    assert_eq!(diff_line("@@ -1,2 +1,2 @@"), DiffLine::Hunk);
    assert_eq!(diff_line("+added"), DiffLine::Add);
    assert_eq!(diff_line("-removed"), DiffLine::Del);
    assert_eq!(diff_line(" same"), DiffLine::Plain);
}

#[test]
fn imported_lines_say_where_they_went() {
    assert_eq!(fate_text("kept"), "Kept in your file");
    assert_eq!(fate_text("regenerated"), "Regenerated from the table");
    assert_eq!(fate_text("profile:code"), "Moved into the table: Code");
}

#[test]
fn what_apply_wrote_is_listed_with_its_tool_and_backup() {
    let m = setup(Shown::Applied);
    assert_eq!(m.stage, Stage::Done);
    assert_eq!(m.written.len(), 4);
    let settings = m
        .written
        .iter()
        .find(|w| w.id == "claude.settings")
        .unwrap();
    assert_eq!(settings.tool, 0);
    assert_eq!(settings.change, Change::Edit);
    assert_eq!(
        settings.backup.as_deref(),
        Some("~/.claude/settings.json.notchling-setup-1793000000000")
    );
    assert_eq!(
        m.written
            .iter()
            .find(|w| w.id == "codex.agents_md")
            .unwrap()
            .tool,
        1
    );
}

#[test]
fn undo_is_offered_for_each_tool_that_was_written_until_it_is_undone() {
    let mut m = setup(Shown::Applied);
    assert_eq!(m.undoable(), [1]);
    assert!(m.undone_for(0).is_some());
    m.finish_undo(1, Ok(json!({"on": true, "results": []})));
    assert!(m.undoable().is_empty());
}

#[test]
fn an_undo_lists_the_files_it_left_alone() {
    let m = setup(Shown::Applied);
    let undone = m.undone_for(0).unwrap();
    assert_eq!(undone.rows.len(), 4);
    let conflicts = undone.conflicts();
    assert_eq!(conflicts.len(), 2);
    assert_eq!(conflicts[0].path, "~/.claude/agents/notchling-builder.md");
    assert_eq!(conflicts[0].outcome, Outcome::Conflict);
    assert_eq!(
        conflicts[0].detail.as_deref(),
        Some("Edited since Notchling wrote it; left as it is")
    );
    assert_eq!(conflicts[1].detail.as_deref(), Some("Yours, kept"));
}

#[test]
fn a_failed_apply_keeps_what_was_done_and_says_why() {
    let mut m = setup(Shown::Setup);
    m.finish_apply(Err((
        "Set Up stopped part way: no space".to_owned(),
        Some(json!({"results": [
            {"id": "claude.rule", "path": "/Users/me/.claude/rules/notchling-cost.md",
                "kind": "create", "done": true, "backup": null, "error": null},
            {"id": "claude.settings", "path": "/Users/me/.claude/settings.json",
                "kind": "update", "done": false, "backup": null, "error": "no space"},
        ]})),
    )));
    assert_eq!(m.stage, Stage::Done);
    assert_eq!(
        m.message.as_deref(),
        Some("Set Up stopped part way: no space")
    );
    assert_eq!(m.written.len(), 2);
    assert!(m.written[0].done);
    assert_eq!(m.written[1].error.as_deref(), Some("no space"));
    assert_eq!(m.undoable(), [0]);
}

#[test]
fn apply_with_the_switch_off_changes_nothing_and_shows_the_daemons_note() {
    let mut m = setup(Shown::Setup);
    m.stage = Stage::Applying;
    m.finish_apply(Ok(
        json!({"on": false, "applied": false, "message": "install is off"}),
    ));
    assert_eq!(m.stage, Stage::Review);
    assert!(m.written.is_empty());
    assert_eq!(m.message.as_deref(), Some("install is off"));
}

#[test]
fn an_unreadable_plan_is_said_and_leaves_the_review() {
    let mut m = SetupModel::new(HOME);
    m.show_plan(Ok(json!({"on": true})), None);
    assert!(m.plan.is_none());
    assert_eq!(m.message.as_deref(), Some("The plan can't be read."));
    let mut m = SetupModel::new(HOME);
    m.show_plan(Err("the daemon isn't running".to_owned()), None);
    assert_eq!(m.message.as_deref(), Some("the daemon isn't running"));
}
