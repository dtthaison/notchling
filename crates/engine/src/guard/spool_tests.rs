use super::event::{Event, Kind};
use super::rules::{Action, Rule, Would};
use super::spool::{append, logged, take, DIR, EXT, MAX_FILES};
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

fn files(dir: &TempDir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.path(DIR))
        .map(|e| {
            e.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
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
    let rules: Vec<_> = taken
        .batches
        .iter()
        .flat_map(|b| b.rows.iter().map(|r| r.rule.as_str()))
        .collect();
    assert_eq!(rules.len(), 3);
    for rule in ["tree", "poll", "read"] {
        assert!(rules.contains(&rule), "{rule}");
    }
    assert_eq!(taken.rows(), 3);
    assert_eq!(files(&dir).len(), 2);
    taken.done();
    assert!(files(&dir).is_empty());
    assert!(take(dir.root()).is_none());
}

#[test]
fn rows_not_confirmed_come_back_under_the_same_id() {
    let dir = TempDir::new("guard-spool");
    append(dir.root(), 1.0, &event(), &[would(Rule::Tree)]);
    let first = take(dir.root()).expect("rows");
    let id = first.batches[0].id.clone();
    drop(first);
    append(dir.root(), 2.0, &event(), &[would(Rule::Poll)]);
    let again = take(dir.root()).expect("rows");
    assert_eq!(again.batches.len(), 2);
    assert_eq!(again.batches.iter().filter(|b| b.id == id).count(), 1);
}

#[test]
fn nothing_to_say_writes_nothing() {
    let dir = TempDir::new("guard-spool");
    assert!(append(dir.root(), 1.0, &event(), &[]));
    assert!(files(&dir).is_empty());
}

#[test]
fn a_queue_nobody_empties_stops_growing() {
    let dir = TempDir::new("guard-spool");
    for i in 0..MAX_FILES {
        dir.write(&format!("{DIR}/{i:08}.{EXT}"), "");
    }
    assert!(!append(dir.root(), 1.0, &event(), &[would(Rule::Tree)]));
    assert_eq!(files(&dir).len(), MAX_FILES);
}

#[test]
fn a_file_still_being_written_is_never_taken() {
    let dir = TempDir::new("guard-spool");
    dir.write(&format!("{DIR}/0001-1-1.tmp"), "{\"ts\":1.0");
    assert!(take(dir.root()).is_none());
    append(dir.root(), 1.0, &event(), &[would(Rule::Tree)]);
    let taken = take(dir.root()).expect("rows");
    assert_eq!(taken.batches.len(), 1);
    taken.done();
    assert!(dir.path(&format!("{DIR}/0001-1-1.tmp")).exists());
}

#[test]
fn hooks_writing_while_the_queue_is_taken_lose_nothing() {
    let dir = TempDir::new("guard-spool");
    let root = dir.root().to_path_buf();
    let writers: Vec<_> = (0..6)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || {
                for _ in 0..40 {
                    assert!(append(&root, 1.0, &event(), &[would(Rule::Tree)]));
                }
            })
        })
        .collect();
    let mut seen = 0;
    while writers.iter().any(|w| !w.is_finished()) {
        if let Some(taken) = take(dir.root()) {
            seen += taken.rows();
            taken.done();
        }
    }
    for w in writers {
        w.join().expect("writer");
    }
    while let Some(taken) = take(dir.root()) {
        seen += taken.rows();
        taken.done();
    }
    assert_eq!(seen, 240);
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
    dir.write(&format!("{DIR}/0001-1-1.{EXT}"), &(lines.join("\n") + "\n"));
    let taken = take(dir.root()).expect("rows");
    assert_eq!(taken.rows(), 1);
}
