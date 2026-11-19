use std::collections::BTreeSet;

use notchling_engine::sessions::AgentGroup;
use notchling_kit::Status;
use serde_json::json;

use super::super::data::Tool;
use super::super::sidebar::sidebar_model;
use super::super::sidebar::tests::session;
use super::{place, row_for, tickets, waiting_rows, Placement, Ticket};
use crate::accounts::store::Snapshot;

const NOW: i64 = 1_000_000_000_000;

fn ticket(id: &str, session: Option<&str>) -> Ticket {
    Ticket {
        id: id.into(),
        session: session.map(str::to_owned),
    }
}

#[test]
fn tickets_come_from_both_lists_and_name_a_session_only_when_it_is_not_blank() {
    let list = tickets(
        &[
            json!({"id": "ap-1", "session": "s-1"}),
            json!({"names": []}),
        ],
        &[
            json!({"id": "r-1"}),
            json!({"id": "r-2", "session": ""}),
            json!({"id": ""}),
        ],
    );
    assert_eq!(
        list,
        vec![
            ticket("ap-1", Some("s-1")),
            ticket("r-1", None),
            ticket("r-2", None)
        ]
    );
}

#[test]
fn a_ticket_finds_its_row_by_the_row_id_or_by_the_transcripts_session_id() {
    let mut named = session("job-a", AgentGroup::Working, None, Tool::Claude);
    named.agent.transcript_path = Some("/p/0c1d-aaaa.jsonl".into());
    let other = session("job-b", AgentGroup::Working, None, Tool::Claude);
    let rows = vec![named, other];
    assert_eq!(row_for(&rows, "job-b").as_deref(), Some("job-b"));
    assert_eq!(row_for(&rows, "0c1d-aaaa").as_deref(), Some("job-a"));
    assert_eq!(row_for(&rows, "nobody"), None);
}

#[test]
fn a_known_session_gets_a_card_and_anything_else_gets_the_sheet() {
    let rows = vec![session("job-a", AgentGroup::Working, None, Tool::Claude)];
    assert_eq!(
        place(&ticket("t1", Some("job-a")), &rows),
        Placement::Card("job-a".into())
    );
    assert_eq!(place(&ticket("t2", Some("gone")), &rows), Placement::Sheet);
    assert_eq!(place(&ticket("t3", None), &rows), Placement::Sheet);
    let list = vec![
        ticket("t1", Some("job-a")),
        ticket("t3", None),
        ticket("t2", Some("gone")),
    ];
    assert_eq!(
        waiting_rows(&list, &rows),
        BTreeSet::from(["job-a".to_owned()])
    );
}

#[test]
fn a_row_waiting_on_the_vault_shows_the_needs_you_marker_like_a_question() {
    let rows = vec![
        session("asker", AgentGroup::Working, None, Tool::Claude),
        session("quiet", AgentGroup::Working, None, Tool::Claude),
    ];
    let model = sidebar_model(&rows, NOW, None, &Snapshot::default(), None);
    let after = model
        .clone()
        .with_vault(&BTreeSet::from(["asker".to_owned()]));
    let status = |m: &super::SidebarModel, id: &str| {
        m.sections
            .iter()
            .flat_map(|s| &s.rows)
            .find(|r| r.id == id)
            .map(|r| r.status)
    };
    assert_eq!(status(&after, "asker"), Some(Status::Needs));
    assert_eq!(status(&after, "quiet"), status(&model, "quiet"));
    assert_eq!(model.clone().with_vault(&BTreeSet::new()), model);
}
