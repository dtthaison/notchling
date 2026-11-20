use super::event::{Event, Kind};
use super::rules::{Action, Rule, Would};
use super::spool::{append, logged, take, FILE, MAX_SPOOL, TAKEN};
use crate::sessions::testutil::TempDir;
use serde_json::json;

fn event() -> Event {
    Event {
        kind: Kind::Pre,
        session: "s1".into(),
        agent: Some("a1".into()),
        tool: "Bash".into(),
        tool_use_id: Some("t1".into()),
        cwd: Some("/secret/place".into()),
        input: json!({"command": "SECRET-COMMAND"}),
    }
}

fn would(rule: Rule) -> Would {
    Would {
        rule,
        action: Action::Note,
        reason: "SECRET-REASON",
    }
}

#[test]
fn a_row_holds_ids_and_names_only() {
    let row = logged(5.0, &event(), &would(Rule::Tree));
    let text = serde_json::to_string(&row).expect("json");
    for secret in ["SECRET", "/secret", "command"] {
        assert!(!text.contains(secret), "{text}");
    }
    assert_eq!(row.session, "s1");
    assert_eq!(row.agent, "a1");
    assert_eq!(row.tool, "Bash");
    assert_eq!(row.rule, "tree");
    assert_eq!(row.action, "note");
}

#[test]
fn an_unknown_tool_is_logged_as_other() {
    let mut e = event();
    e.tool = "mcp__secret__tool".into();
    assert_eq!(logged(1.0, &e, &would(Rule::Tree)).tool, "other");
    e.tool = "Task".into();
    assert_eq!(logged(1.0, &e, &would(Rule::Tree)).tool, "Agent");
}

#[test]
fn rows_queue_and_are_taken_once() {
    let dir = TempDir::new("guard-spool");
    assert!(take(dir.root()).is_none());
    assert!(append(
        dir.root(),
        1.0,
        &event(),
        &[would(Rule::Tree), would(Rule::Poll)]
    ));
    assert!(append(dir.root(), 2.0, &event(), &[would(Rule::Read)]));
    let taken = take(dir.root()).expect("rows");
    assert_eq!(
        taken
            .rows
            .iter()
            .map(|r| r.rule.as_str())
            .collect::<Vec<_>>(),
        vec!["tree", "poll", "read"]
    );
    assert!(!dir.path(FILE).exists());
    assert!(dir.path(TAKEN).exists());
    taken.done();
    assert!(take(dir.root()).is_none());
}

#[test]
fn rows_not_confirmed_come_back() {
    let dir = TempDir::new("guard-spool");
    append(dir.root(), 1.0, &event(), &[would(Rule::Tree)]);
    let first = take(dir.root()).expect("rows");
    drop(first);
    append(dir.root(), 2.0, &event(), &[would(Rule::Poll)]);
    let again = take(dir.root()).expect("rows");
    assert_eq!(again.rows.len(), 1);
    assert_eq!(again.rows[0].rule, "tree");
    again.done();
    let last = take(dir.root()).expect("rows");
    assert_eq!(last.rows[0].rule, "poll");
}

#[test]
fn nothing_to_say_writes_nothing() {
    let dir = TempDir::new("guard-spool");
    assert!(append(dir.root(), 1.0, &event(), &[]));
    assert!(!dir.path(FILE).exists());
}

#[test]
fn a_queue_nobody_empties_stops_growing() {
    let dir = TempDir::new("guard-spool");
    dir.write(FILE, &"x".repeat(MAX_SPOOL as usize));
    assert!(!append(dir.root(), 1.0, &event(), &[would(Rule::Tree)]));
    assert_eq!(
        std::fs::metadata(dir.path(FILE)).expect("meta").len(),
        MAX_SPOOL
    );
}

#[test]
fn rows_that_do_not_belong_are_dropped_when_read() {
    let dir = TempDir::new("guard-spool");
    let good = json!({"ts": 1.0, "session": "s1", "agent": "", "tool": "Bash", "rule": "tree", "action": "note"});
    let lines = [
        good.to_string(),
        "not json".to_string(),
        json!({"ts": 1.0, "session": "s 1", "agent": "", "tool": "Bash", "rule": "tree", "action": "note"}).to_string(),
        json!({"ts": 1.0, "session": "s1", "agent": "", "tool": "rm -rf /", "rule": "tree", "action": "note"}).to_string(),
        json!({"ts": 1.0, "session": "s1", "agent": "", "tool": "Bash", "rule": "secret", "action": "note"}).to_string(),
        json!({"ts": 1.0, "session": "s1", "agent": "", "tool": "Bash", "rule": "tree", "action": "later"}).to_string(),
        json!({"ts": 1.0, "session": "s1", "agent": "x y", "tool": "Bash", "rule": "tree", "action": "note"}).to_string(),
    ];
    dir.write(FILE, &(lines.join("\n") + "\n"));
    let taken = take(dir.root()).expect("rows");
    assert_eq!(taken.rows.len(), 1);
}
