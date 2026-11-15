use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};

use super::remote::FOLLOW_EVERY_MS;
use super::remote_follow::{
    retry_wait, Follow, FOLLOW_RETRY_MS, FOLLOW_RETRY_SLOW_MS, QUICK_REFUSALS, SLOW_REFUSALS,
};
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
    let request = follow.begin(REMOTE, 1_000);
    assert!(!follow.due(Some(&row), 1_000 + FOLLOW_EVERY_MS - 1));
    follow.finish(request, true, 2_000);
    assert!(!follow.due(Some(&row), 2_000 + FOLLOW_EVERY_MS - 1));
    assert!(follow.due(Some(&row), 2_000 + FOLLOW_EVERY_MS));
    assert_eq!(FOLLOW_EVERY_MS, 60_000);
}

#[test]
fn a_refused_follow_is_tried_again_after_five_seconds() {
    let mut follow = Follow::default();
    let row = session(REMOTE);
    let request = follow.begin(REMOTE, 1_000);
    follow.finish(request, false, 1_100);
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
    let first = follow.begin(REMOTE, 100);
    follow.finish(first, true, 150);
    let other = session("00000000000000a1/j2");
    assert!(follow.due(Some(&other), 200));
    let second = follow.begin("00000000000000a1/j2", 200);
    follow.finish(first, false, 210);
    assert!(
        !follow.due(Some(&other), 220),
        "the answer for the first row changed nothing"
    );
    follow.finish(second, false, 230);
    assert!(follow.due(Some(&other), 230 + FOLLOW_RETRY_MS));
}

#[test]
fn a_stale_answer_for_the_same_row_does_not_settle_the_newer_call() {
    let mut follow = Follow::default();
    let row = session(REMOTE);
    let other = session("00000000000000a1/j2");
    let old = follow.begin(REMOTE, 0);
    assert!(follow.due(Some(&other), 10));
    follow.begin("00000000000000a1/j2", 10);
    assert!(follow.due(Some(&row), 20));
    let newer = follow.begin(REMOTE, 20);
    assert_ne!(old, newer);
    follow.finish(old, false, 30);
    assert!(
        !follow.due(Some(&row), 30 + FOLLOW_RETRY_MS),
        "the first call's refusal changed nothing while the newer one is out"
    );
    follow.finish(newer, true, 40);
    assert!(!follow.due(Some(&row), 40 + FOLLOW_RETRY_MS));
    assert!(follow.due(Some(&row), 40 + FOLLOW_EVERY_MS));
    follow.finish(old, false, 50);
    assert!(
        !follow.due(Some(&row), 40 + FOLLOW_EVERY_MS - 1),
        "an answer after the newer one settled changed nothing"
    );
}

#[test]
fn a_reply_to_a_call_that_was_repeated_after_a_minute_is_ignored() {
    let mut follow = Follow::default();
    let row = session(REMOTE);
    let lost = follow.begin(REMOTE, 0);
    assert!(follow.due(Some(&row), FOLLOW_EVERY_MS));
    let again = follow.begin(REMOTE, FOLLOW_EVERY_MS);
    follow.finish(lost, false, FOLLOW_EVERY_MS + 1);
    assert!(!follow.due(Some(&row), FOLLOW_EVERY_MS + 1 + FOLLOW_RETRY_MS));
    follow.finish(again, true, FOLLOW_EVERY_MS + 2);
    assert!(!follow.due(Some(&row), FOLLOW_EVERY_MS + 2 + FOLLOW_RETRY_MS));
}

fn refuse(follow: &mut Follow, now: i64) {
    let request = follow.begin(REMOTE, now);
    follow.finish(request, false, now);
}

#[test]
fn a_session_that_keeps_refusing_is_asked_less_and_less_often() {
    let mut follow = Follow::default();
    let row = session(REMOTE);
    let mut now = 0;
    let mut gaps = Vec::new();
    for _ in 0..10 {
        refuse(&mut follow, now);
        let mut waited = 0;
        while !follow.due(Some(&row), now + waited) {
            waited += 1_000;
        }
        gaps.push(waited);
        now += waited;
    }
    assert_eq!(
        gaps,
        [5, 5, 5, 15, 15, 15, 60, 60, 60, 60].map(|s| s * 1_000)
    );
    assert_eq!(QUICK_REFUSALS, 3);
    assert_eq!(SLOW_REFUSALS, 6);
    assert_eq!(FOLLOW_RETRY_SLOW_MS, 15_000);
}

#[test]
fn the_backoff_steps_are_five_fifteen_and_sixty_seconds() {
    assert_eq!(retry_wait(1), 5_000);
    assert_eq!(retry_wait(QUICK_REFUSALS), 5_000);
    assert_eq!(retry_wait(QUICK_REFUSALS + 1), 15_000);
    assert_eq!(retry_wait(SLOW_REFUSALS), 15_000);
    assert_eq!(retry_wait(SLOW_REFUSALS + 1), 60_000);
    assert_eq!(retry_wait(u32::MAX), 60_000);
}

#[test]
fn an_answer_resets_the_backoff() {
    let mut follow = Follow::default();
    let row = session(REMOTE);
    for step in 0..5 {
        refuse(&mut follow, step * 100_000);
    }
    let request = follow.begin(REMOTE, 600_000);
    follow.finish(request, true, 600_000);
    let request = follow.begin(REMOTE, 700_000);
    follow.finish(request, false, 700_000);
    assert!(!follow.due(Some(&row), 700_000 + FOLLOW_RETRY_MS - 1));
    assert!(follow.due(Some(&row), 700_000 + FOLLOW_RETRY_MS));
}

#[test]
fn changing_the_row_resets_the_backoff() {
    let mut follow = Follow::default();
    let row = session(REMOTE);
    for step in 0..5 {
        refuse(&mut follow, step * 100_000);
    }
    let other = follow.begin("00000000000000a1/j2", 600_000);
    follow.finish(other, true, 600_000);
    let back = follow.begin(REMOTE, 610_000);
    follow.finish(back, false, 610_000);
    assert!(follow.due(Some(&row), 610_000 + FOLLOW_RETRY_MS));
}
