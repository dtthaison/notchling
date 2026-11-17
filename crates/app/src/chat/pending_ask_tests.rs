use notchling_engine::sessions::AgentGroup;

use super::super::data::Tool;
use super::super::sidebar::tests::session;
use super::super::sidebar::Pending;
use super::*;

fn sessions() -> Vec<Session> {
    vec![
        session("row-a", AgentGroup::Working, None, Tool::Claude),
        session("row-b", AgentGroup::Completed, None, Tool::Claude),
    ]
}

#[test]
fn the_row_is_found_by_the_agents_outbox_key() {
    let all = sessions();
    let key = all[1].agent.outbox_key();
    assert_eq!(row_for_key(&all, &key).as_deref(), Some("row-b"));
    assert_eq!(row_for_key(&all, "nobody"), None);
    assert_eq!(row_for_key(&[], &key), None);
}

#[test]
fn a_cold_open_waits_for_the_load_then_gives_up() {
    let pending = PendingAsk::new("row-b", 1_000);
    assert_eq!(pending.after_load(&[], 1_001), Pending::Waiting);
    assert_eq!(
        pending.after_load(&sessions(), 2_000),
        Pending::Found("row-b".into())
    );
    assert_eq!(
        pending.after_load(&[], 1_000 + PendingAsk::WAIT_MS),
        Pending::GaveUp
    );
}
