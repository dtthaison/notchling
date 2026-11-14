use notchling_engine::transcript::subagents::{Origin, Status, Subagent};

use super::{PendingChild, Step};

fn child(id: &str) -> Subagent {
    Subagent {
        id: id.into(),
        path: String::new(),
        description: String::new(),
        agent_type: String::new(),
        model: None,
        effort: None,
        background: false,
        tool_use_id: None,
        started: 0.0,
        updated: 0.0,
        status: Status::Completed,
        origin: Origin::default(),
        session: String::new(),
        parent: String::new(),
        depth: 1,
    }
}

#[test]
fn the_row_is_opened_once_the_parents_children_are_listed() {
    let mut pending = PendingChild::new("codex:kid");
    assert_eq!(pending.after_load(&[]), Step::Waiting);
    assert_eq!(
        pending.after_load(&[child("codex:other"), child("codex:kid")]),
        Step::Open("codex:kid".into())
    );
}

#[test]
fn a_child_that_never_shows_up_is_given_up_on() {
    let mut pending = PendingChild::new("codex:kid");
    for _ in 1..PendingChild::LOADS {
        assert_eq!(pending.after_load(&[]), Step::Waiting);
    }
    assert_eq!(pending.after_load(&[]), Step::GaveUp);
}
