use super::*;
use notchling_engine::sessions::AgentTarget;
use notchling_engine::transcript::snapshot::{ChatLine, LineKind};
use std::collections::HashMap;

fn agent(id: &str, since_ms: i64) -> Agent {
    Agent {
        id: id.into(),
        name: id.into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms,
        target: AgentTarget::Job(format!("job-{id}")),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
    }
}

fn snapshot(text: &str) -> Snapshot {
    Snapshot {
        lines: vec![ChatLine {
            id: 0,
            kind: LineKind::Assistant,
            text: text.into(),
        }],
        ..Snapshot::default()
    }
}

fn poll(model: &mut NotchModel, agents: Vec<Agent>, snapshots: HashMap<String, Snapshot>) {
    model.update(agents, snapshots, HashMap::new());
}

#[test]
fn the_same_poll_twice_draws_nothing_new() {
    let mut model = NotchModel::default();
    let snapshots = HashMap::from([("a".to_owned(), snapshot("one"))]);
    poll(
        &mut model,
        vec![agent("a", 0), agent("b", 0)],
        snapshots.clone(),
    );
    let before = model.drawn();
    poll(&mut model, vec![agent("a", 0), agent("b", 0)], snapshots);
    assert_eq!(model.drawn(), before);
}

#[test]
fn the_notch_redraws_when_an_age_changes() {
    let mut model = NotchModel::default();
    poll(&mut model, vec![agent("a", 0)], HashMap::new());
    model.now_ms = 30_000;
    let before = model.drawn();
    model.now_ms = 45_000;
    assert_eq!(model.drawn(), before);
    model.now_ms = 61_000;
    assert_ne!(model.drawn(), before);
}

#[test]
fn a_new_snapshot_line_is_a_change() {
    let mut model = NotchModel::default();
    poll(
        &mut model,
        vec![agent("a", 0)],
        HashMap::from([("a".to_owned(), snapshot("one"))]),
    );
    let before = model.drawn();
    poll(
        &mut model,
        vec![agent("a", 0)],
        HashMap::from([("a".to_owned(), snapshot("two"))]),
    );
    assert_ne!(model.drawn(), before);
}

#[test]
fn a_peek_is_a_change() {
    let mut model = NotchModel::default();
    poll(&mut model, vec![agent("a", 0)], HashMap::new());
    let before = model.drawn();
    model.peek("a", 0);
    assert_ne!(model.drawn(), before);
}
