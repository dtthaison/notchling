use std::collections::BTreeSet;

use notchling_engine::sessions::AgentGroup;
use notchling_kit::Status;
use serde_json::json;

use super::super::data::Tool;
use super::super::sidebar::sidebar_model;
use super::super::sidebar::tests::session;
use super::{pick, place, row_for, tickets, visible, waiting_rows, Covers, Placement, Ticket};
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
fn a_ticket_finds_its_row_by_the_session_id_only_and_only_when_exactly_one_row_has_it() {
    let mut named = session("job-a", AgentGroup::Working, None, Tool::Claude);
    named.agent.transcript_path = Some("/p/0c1d-aaaa.jsonl".into());
    let other = session("job-b", AgentGroup::Working, None, Tool::Claude);
    let rows = vec![named.clone(), other];
    assert_eq!(row_for(&rows, "0c1d-aaaa").as_deref(), Some("job-a"));
    assert_eq!(
        row_for(&rows, "job-b"),
        None,
        "a row id is not a session id"
    );
    assert_eq!(row_for(&rows, "nobody"), None);
    let mut twin = session("job-c", AgentGroup::Working, None, Tool::Claude);
    twin.agent.transcript_path = Some("/q/0c1d-aaaa.jsonl".into());
    let both = vec![named, twin];
    assert_eq!(row_for(&both, "0c1d-aaaa"), None, "two rows, so no card");
    let mut by_file = session("job-d", AgentGroup::Working, None, Tool::Claude);
    by_file.transcript = Some("/r/feed-1234.jsonl".into());
    assert_eq!(row_for(&[by_file], "feed-1234").as_deref(), Some("job-d"));
}

fn covers() -> Covers {
    Covers {
        active: true,
        ..Covers::default()
    }
}

#[test]
fn a_prompt_counts_as_visible_only_when_nothing_covers_it_and_the_window_is_in_front() {
    let card = Placement::Card("row".into());
    assert!(visible(&card, Some("row"), true, &covers()));
    assert!(!visible(&card, Some("other"), true, &covers()));
    assert!(
        !visible(&card, Some("row"), false, &covers()),
        "not in front"
    );
    let each = [
        Covers {
            active: false,
            ..covers()
        },
        Covers {
            sheet: true,
            ..covers()
        },
        Covers {
            inbox: true,
            ..covers()
        },
        Covers {
            phone: true,
            ..covers()
        },
        Covers {
            subagent: true,
            ..covers()
        },
        Covers {
            terminal: true,
            ..covers()
        },
        Covers {
            vault_sheet: true,
            ..covers()
        },
    ];
    for c in each {
        assert!(!visible(&card, Some("row"), true, &c), "{c:?}");
    }
    assert!(visible(&Placement::Sheet, None, true, &covers()));
    assert!(visible(
        &Placement::Sheet,
        None,
        true,
        &Covers {
            vault_sheet: true,
            sheet: true,
            terminal: true,
            ..covers()
        }
    ));
    assert!(!visible(
        &Placement::Sheet,
        None,
        true,
        &Covers {
            inbox: true,
            ..covers()
        }
    ));
    assert!(!visible(
        &Placement::Sheet,
        None,
        true,
        &Covers {
            phone: true,
            ..covers()
        }
    ));
    assert!(!visible(
        &Placement::Sheet,
        None,
        true,
        &Covers {
            active: false,
            ..covers()
        }
    ));
    assert!(!visible(&Placement::Sheet, None, false, &covers()));
}

#[test]
fn the_raised_ticket_is_shown_first_and_otherwise_the_oldest() {
    assert_eq!(pick(&[], Some("a")), None);
    assert_eq!(pick(&["a", "b", "c"], None), Some(0));
    assert_eq!(pick(&["a", "b", "c"], Some("c")), Some(2));
    assert_eq!(pick(&["a", "b"], Some("gone")), Some(0));
}

#[test]
fn a_known_session_gets_a_card_and_anything_else_gets_the_sheet() {
    let mut a = session("job-a", AgentGroup::Working, None, Tool::Claude);
    a.agent.transcript_path = Some("/p/sess-a.jsonl".into());
    let rows = vec![a];
    assert_eq!(
        place(&ticket("t1", Some("sess-a")), &rows),
        Placement::Card("job-a".into())
    );
    assert_eq!(place(&ticket("t2", Some("gone")), &rows), Placement::Sheet);
    assert_eq!(place(&ticket("t3", None), &rows), Placement::Sheet);
    let list = vec![
        ticket("t1", Some("sess-a")),
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
