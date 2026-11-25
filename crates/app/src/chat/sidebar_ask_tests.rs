use notchling_engine::sessions::AgentGroup;
use notchling_kit::Status;
use serde_json::json;

use super::super::asks::AskFeed;
use super::super::data::Tool;
use super::super::sidebar::tests::session;
use super::super::sidebar::{sidebar_model, SidebarModel};
use crate::accounts::store::Snapshot;

const NOW: i64 = 1_000_000_000_000;

fn feed(rows: &[(&str, &str, &str)]) -> AskFeed {
    let mut feed = AskFeed::default();
    for (id, session, state) in rows {
        let wire = json!({
            "id": id, "session": session, "asker": "main", "question": "Which?",
            "choices": [], "mainMayAnswer": false, "state": state, "waiting": true,
            "createdMs": 1, "updatedMs": 1,
        });
        feed.apply("ask.changed", &json!({"row": wire}));
    }
    feed
}

fn statuses(model: &SidebarModel) -> Vec<(String, Status)> {
    model
        .sections
        .iter()
        .flat_map(|s| &s.rows)
        .map(|r| (r.id.clone(), r.status))
        .collect()
}

#[test]
fn a_row_with_an_open_question_shows_the_needs_you_marker() {
    let sessions = vec![
        session("asker", AgentGroup::Working, None, Tool::Claude),
        session("quiet", AgentGroup::Working, None, Tool::Claude),
        session("old", AgentGroup::Completed, None, Tool::Claude),
    ];
    let key = sessions[0].agent.outbox_key();
    let old = sessions[2].agent.outbox_key();
    let feed = feed(&[("q1", &key, "open"), ("q2", &old, "answered")]);
    let model = sidebar_model(&sessions, NOW, None, &Snapshot::default(), None);
    let before = statuses(&model);
    let after = statuses(&model.with_asks(&feed, &sessions));
    let marked: Vec<_> = after
        .iter()
        .filter(|(id, s)| *s == Status::Needs && !before.contains(&(id.clone(), *s)))
        .map(|(id, _)| id.as_str())
        .collect();
    assert_eq!(marked, ["asker"]);
    assert!(after.contains(&("quiet".to_owned(), Status::Working)));
    assert!(after.contains(&("old".to_owned(), Status::Done)));
}

#[test]
fn no_questions_leave_the_model_as_it_was() {
    let sessions = vec![session("a", AgentGroup::Working, None, Tool::Claude)];
    let model = sidebar_model(&sessions, NOW, None, &Snapshot::default(), None);
    assert_eq!(
        model.clone().with_asks(&AskFeed::default(), &sessions),
        model
    );
}
