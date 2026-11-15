use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};

use super::remote::FOLLOW_EVERY_MS;
use super::remote_follow::{Follow, FOLLOW_RETRY_MS};
use crate::chat::data::{Session, Tool};

const REMOTE: &str = "00000000000000a1/j1";

fn session(id: &str) -> Session {
    Session {
        agent: Agent {
            id: id.into(),
            name: "job".into(),
            project: "api".into(),
            cwd: "/srv/api".into(),
            group: AgentGroup::Working,
            label: "Working".into(),
            detail: None,
            since_ms: 1,
            target: AgentTarget::Job("j1".into()),
            suggested_reply: None,
            transcript_path: None,
            host: "Background".into(),
            headless: false,
            config_dir: None,
            command: None,
            resume_session_id: None,
            at_prompt: false,
            node: None,
        },
        tool: Tool::Claude,
        transcript: None,
    }
}

#[test]
fn only_a_selected_remote_row_is_followed() {
    let follow = Follow::default();
    assert!(!follow.due(None, 0));
    assert!(!follow.due(Some(&session("j1")), 0));
    assert!(follow.due(Some(&session(REMOTE)), 0));
}

#[test]
fn a_followed_row_is_renewed_each_minute_after_the_answer() {
    let mut follow = Follow::default();
    let row = session(REMOTE);
    follow.begin(REMOTE, 1_000);
    assert!(!follow.due(Some(&row), 1_000 + FOLLOW_EVERY_MS - 1));
    follow.finish(REMOTE, true, 2_000);
    assert!(!follow.due(Some(&row), 2_000 + FOLLOW_EVERY_MS - 1));
    assert!(follow.due(Some(&row), 2_000 + FOLLOW_EVERY_MS));
    assert_eq!(FOLLOW_EVERY_MS, 60_000);
}

#[test]
fn a_refused_follow_is_tried_again_after_five_seconds() {
    let mut follow = Follow::default();
    let row = session(REMOTE);
    follow.begin(REMOTE, 1_000);
    follow.finish(REMOTE, false, 1_100);
    assert!(!follow.due(Some(&row), 1_100 + FOLLOW_RETRY_MS - 1));
    assert!(follow.due(Some(&row), 1_100 + FOLLOW_RETRY_MS));
    assert_eq!(FOLLOW_RETRY_MS, 5_000);
}

#[test]
fn a_call_still_out_is_not_repeated_and_a_lost_one_is_after_a_minute() {
    let mut follow = Follow::default();
    let row = session(REMOTE);
    follow.begin(REMOTE, 0);
    assert!(!follow.due(Some(&row), FOLLOW_RETRY_MS * 3));
    assert!(follow.due(Some(&row), FOLLOW_EVERY_MS));
}

#[test]
fn another_row_is_followed_at_once_and_a_late_answer_for_the_old_one_is_ignored() {
    let mut follow = Follow::default();
    follow.begin(REMOTE, 100);
    follow.finish(REMOTE, true, 150);
    let other = session("00000000000000a1/j2");
    assert!(follow.due(Some(&other), 200));
    follow.begin("00000000000000a1/j2", 200);
    follow.finish(REMOTE, false, 210);
    assert!(
        !follow.due(Some(&other), 220),
        "the answer for the first row changed nothing"
    );
    follow.finish("00000000000000a1/j2", false, 230);
    assert!(follow.due(Some(&other), 230 + FOLLOW_RETRY_MS));
}
