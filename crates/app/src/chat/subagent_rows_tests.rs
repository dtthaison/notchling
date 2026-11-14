use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::chat::inspector::{inspector, no_writers};
use crate::testutil::Scratch;
use notchling_engine::transcript::subagents::Origin;

#[derive(Clone, Default)]
struct Recorder {
    read: Rc<RefCell<Vec<String>>>,
}

impl Peek for Recorder {
    fn read(&self, agent: &Subagent) -> Facts {
        self.read.borrow_mut().push(agent.id.clone());
        Facts {
            context: Some(232_300),
            doing: agent.is_running().then(|| "Running tests…".to_owned()),
        }
    }
}

fn world(s: &Scratch, total: usize, running: &[usize]) -> Vec<Subagent> {
    (0..total)
        .map(|n| {
            let file = s.write(&format!("agent-a{n}.jsonl"), &format!("line {n}\n"));
            Subagent {
                id: format!("a{n}"),
                path: file.to_string_lossy().into_owned(),
                description: format!("job {n}"),
                agent_type: "Explore".into(),
                model: Some("sonnet".into()),
                effort: Some("high".into()),
                background: false,
                tool_use_id: None,
                started: n as f64,
                updated: n as f64,
                status: if running.contains(&n) {
                    SubagentStatus::Running
                } else {
                    SubagentStatus::Completed
                },
                origin: Default::default(),
                session: String::new(),
                parent: String::new(),
                depth: 1,
            }
        })
        .collect()
}

fn rows_of(
    rows: &mut SubagentRows<Recorder>,
    agents: &[Subagent],
    shown: &[usize],
) -> Vec<SubagentRow> {
    rows.rows(agents, shown)
}

fn taken(recorder: &Recorder) -> Vec<String> {
    std::mem::take(&mut *recorder.read.borrow_mut())
}

#[test]
fn only_the_shown_rows_are_read() {
    let s = Scratch::new("rows-shown");
    let agents = world(&s, 40, &[5, 31]);
    let recorder = Recorder::default();
    let mut rows = SubagentRows::new(recorder.clone());
    let got = rows_of(&mut rows, &agents, &[31, 5]);
    assert_eq!(taken(&recorder), ["a31", "a5"]);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].detail, "Running · sonnet high · Explore · 232.3k");
    assert_eq!(got[0].doing.as_deref(), Some("Running tests…"));
    assert_eq!(got[0].id, "a31");
}

#[test]
fn a_finished_row_gets_its_number_and_no_doing_line() {
    let s = Scratch::new("rows-done");
    let agents = world(&s, 4, &[]);
    let mut rows = SubagentRows::new(Recorder::default());
    let got = rows_of(&mut rows, &agents, &[3]);
    assert_eq!(got[0].detail, "Done · sonnet high · Explore · 232.3k");
    assert_eq!(got[0].doing, None);
}

#[test]
fn an_unchanged_file_is_never_read_again_and_a_changed_one_is() {
    let s = Scratch::new("rows-stamp");
    let agents = world(&s, 6, &[1]);
    let recorder = Recorder::default();
    let mut rows = SubagentRows::new(recorder.clone());
    let shown = [5, 4, 3, 1];
    let first = rows_of(&mut rows, &agents, &shown);
    assert_eq!(taken(&recorder).len(), 4);
    for _ in 0..3 {
        assert_eq!(rows_of(&mut rows, &agents, &shown), first);
    }
    assert!(taken(&recorder).is_empty(), "nothing changed, nothing read");

    s.write("agent-a1.jsonl", "line 1\nand a longer second line\n");
    rows_of(&mut rows, &agents, &shown);
    assert_eq!(taken(&recorder), ["a1"], "only the one that changed");
}

#[test]
fn revealing_more_reads_only_the_ones_just_revealed() {
    let s = Scratch::new("rows-more");
    let agents = world(&s, 40, &[]);
    let recorder = Recorder::default();
    let mut rows = SubagentRows::new(recorder.clone());
    rows_of(&mut rows, &agents, &[39, 38, 37]);
    assert_eq!(taken(&recorder).len(), 3);
    let next: Vec<usize> = (27..40).rev().collect();
    rows_of(&mut rows, &agents, &next);
    let read = taken(&recorder);
    assert_eq!(read.len(), 10);
    assert!(!read.contains(&"a39".to_owned()));
}

#[test]
fn a_finished_one_that_runs_again_with_the_same_file_is_read_for_its_doing_line() {
    let s = Scratch::new("rows-flip");
    let mut agents = world(&s, 2, &[]);
    let recorder = Recorder::default();
    let mut rows = SubagentRows::new(recorder.clone());
    let before = rows_of(&mut rows, &agents, &[1]);
    assert_eq!(before[0].doing, None);
    agents[1].status = SubagentStatus::Running;
    let after = rows_of(&mut rows, &agents, &[1]);
    assert_eq!(after[0].doing.as_deref(), Some("Running tests…"));
    assert_eq!(taken(&recorder), ["a1", "a1"]);
}

#[test]
fn a_row_without_a_file_or_from_antigravity_is_never_read() {
    let s = Scratch::new("rows-none");
    let mut agents = world(&s, 3, &[]);
    agents[0].path.clear();
    agents[1].origin = Origin::native(Tool::Antigravity);
    agents[2].path = s.path().join("gone.jsonl").to_string_lossy().into_owned();
    let recorder = Recorder::default();
    let mut rows = SubagentRows::new(recorder.clone());
    let got = rows_of(&mut rows, &agents, &[0, 1, 2]);
    assert!(taken(&recorder).is_empty());
    assert_eq!(got[0].detail, "Done · sonnet high · Explore");
    assert_eq!(got[2].detail, "Done · sonnet high · Explore");
}

#[test]
fn the_details_rows_do_not_need_the_files_once_read() {
    let s = Scratch::new("rows-gone");
    let agents = world(&s, 4, &[2]);
    let mut rows = SubagentRows::new(Recorder::default());
    let got = rows_of(&mut rows, &agents, &[3, 2, 1]);
    let conversation = Conversation::default();
    let with_files = inspector(&conversation, &got, &no_writers);
    for agent in &agents {
        std::fs::remove_file(&agent.path).expect("remove");
    }
    let without = inspector(&conversation, &got, &no_writers);
    assert_eq!(with_files, without);
    assert_eq!(
        without.subagents[0].detail,
        "Done · sonnet high · Explore · 232.3k"
    );
}

#[test]
fn what_a_row_remembers_goes_when_its_agent_does() {
    let s = Scratch::new("rows-prune");
    let agents = world(&s, 3, &[]);
    let recorder = Recorder::default();
    let mut rows = SubagentRows::new(recorder.clone());
    rows_of(&mut rows, &agents, &[0, 1, 2]);
    assert_eq!(rows.kept.len(), 3);
    rows_of(&mut rows, &agents[..1], &[0]);
    assert_eq!(rows.kept.len(), 1);
}

#[test]
fn a_native_codex_child_is_not_handed_over() {
    let s = Scratch::new("rows-native-codex");
    let mut agents = world(&s, 2, &[]);
    agents[0].origin = Origin::native(Tool::Codex);
    agents[0].agent_type = "worker".into();
    agents[1].origin = Origin::delegated(Tool::Codex);
    agents[1].agent_type = "Codex".into();
    let mut rows = SubagentRows::new(Recorder::default());
    let got = rows_of(&mut rows, &agents, &[0, 1]);
    assert_eq!(got[0].tool, ToolMark::Codex);
    assert_eq!(got[0].detail, "Done · sonnet high · worker · 232.3k");
    assert_eq!(got[1].tool, ToolMark::Codex);
    assert_eq!(got[1].detail, "Done · Codex · sonnet high · 232.3k");
}
