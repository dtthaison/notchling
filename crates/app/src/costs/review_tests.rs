use serde_json::json;

use super::check::{CheckModel, Dot, Drift};
use super::profile::Profile;
use super::renders::{check, plan_reply, profile_reply, setup, versions_reply, Shown, HOME};
use super::setup::{why_text, Plan, Stage};

fn plan() -> Plan {
    Plan::from_reply(&plan_reply(true, false), Some(&versions_reply()), HOME).unwrap()
}

#[test]
fn a_rule_file_and_a_pointer_say_what_will_happen_to_them() {
    let p = plan();
    let rules = p.action("claude.rules.replaced.model-choice.md").unwrap();
    let words = rules.explain().join(" ");
    assert!(words.contains("imported into notchling-cost.md"), "{words}");
    assert!(words.contains("replaced"), "{words}");
    assert!(!words.contains("pointer line stays"), "{words}");
    let pointer = p.action("claude.pointer.agents/CLAUDE.md").unwrap();
    assert!(pointer.explain().join(" ").contains("old rule file"));
}

#[test]
fn every_import_is_listed_under_its_own_source_file() {
    let p = plan();
    let sources: Vec<&str> = p.imports.iter().map(|i| i.source.as_str()).collect();
    assert_eq!(
        sources,
        [
            "/Users/me/.claude/rules/model-choice.md",
            "/Users/me/.claude/rules/costs-old.md"
        ]
    );
    assert_eq!(p.imports[0].lines.len(), 4);
    assert_eq!(p.imports[1].lines[0].fate, "profile:judge");
}

#[test]
fn a_plan_without_imports_falls_back_to_its_flat_report() {
    let mut reply = plan_reply(true, false);
    reply["plan"]["profile"] = json!({});
    reply["plan"]["report"] = json!([{"line": 2, "text": "- a", "fate": "kept"}]);
    let p = Plan::from_reply(&reply, None, HOME).unwrap();
    assert_eq!(p.imports.len(), 1);
    assert_eq!(p.imports[0].source, "");
    assert_eq!(p.imports[0].lines[0].number, 2);
}

#[test]
fn every_import_of_the_profile_is_read_and_imported_is_the_first() {
    let p = Profile::from_reply(&profile_reply(false)).unwrap();
    assert_eq!(p.imports.len(), 2);
    assert_eq!(
        p.imports[0].source,
        "/Users/me/.claude/rules/model-choice.md"
    );
    let mut reply = profile_reply(false);
    reply["profile"].as_object_mut().unwrap().remove("imports");
    let older = Profile::from_reply(&reply).unwrap();
    assert_eq!(older.imports.len(), 1);
    assert_eq!(older.imports[0], p.imports[0]);
}

#[test]
fn a_damaged_record_leaves_no_plan_so_there_is_nothing_to_apply() {
    let m = setup(Shown::Damaged);
    assert!(m.plan.is_none());
    assert!(!m.can_apply());
    assert!(m.message.as_deref().unwrap().contains("damaged"));
}

#[test]
fn an_apply_the_daemon_failed_with_no_results_returns_to_the_error() {
    let mut m = setup(Shown::Setup);
    m.stage = Stage::Applying;
    m.finish_apply(Err((
        "the setup record is damaged; nothing was changed.".to_owned(),
        None,
    )));
    assert_eq!(m.stage, Stage::Review);
    assert!(m.plan.is_none());
    assert!(!m.can_apply());
    assert!(m.written.is_empty());
    assert!(m.message.as_deref().unwrap().contains("damaged"));
}

#[test]
fn the_engine_s_reasons_are_said_in_plain_words() {
    assert_eq!(why_text("kept"), "Yours, not managed");
    assert_eq!(
        why_text("agents.max_threads: kept; a.b: changed-since"),
        "agents.max_threads: Yours, not managed; a.b: changed since Notchling wrote it, left as it is"
    );
    assert_eq!(why_text("unreadable: no access"), "unreadable: no access");
}

#[test]
fn what_the_person_took_over_reads_as_yours_not_managed() {
    let m = check(Shown::Check);
    let config = m.rows.iter().find(|r| r.id == "codex.config").unwrap();
    assert_eq!(config.state, Drift::Yours);
    assert_eq!(config.detail, None);
    let pstack = m.rows.iter().find(|r| r.id == "claude.pstack").unwrap();
    assert_eq!(pstack.state, Drift::Yours);
    assert_eq!(Drift::Yours.dot(), Dot::Muted);
    assert!(Drift::Yours.resolutions().is_empty());
    assert_eq!(m.drifted(), 4);
}

#[test]
fn a_changed_key_among_kept_ones_is_still_a_change() {
    let mut m = CheckModel::default();
    m.show(
        Ok(json!({"on": true, "drift": true, "rows": [
            {"id": "codex.config", "tool": "codex", "path": "/x", "state": "changed_by_tool",
                "detail": "a: kept; b: changed-since"},
        ]})),
        HOME,
    );
    assert_eq!(m.rows[0].state, Drift::ChangedByTool);
    assert!(m.rows[0]
        .detail
        .as_deref()
        .unwrap()
        .contains("Yours, not managed"));
}

#[test]
fn a_damaged_record_leaves_no_rows_to_keep_or_restore() {
    let mut m = check(Shown::Check);
    m.show(Err("the setup record is damaged".to_owned()), HOME);
    assert!(m.rows.is_empty());
    assert!(!m.can_resolve());
    assert_eq!(m.message.as_deref(), Some("the setup record is damaged"));
}
