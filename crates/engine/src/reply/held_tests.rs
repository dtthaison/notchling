use super::*;
use crate::sessions::agent::sample;
use crate::sessions::{AgentGroup, AgentTarget};

fn row(id: &str, group: AgentGroup) -> Agent {
    let mut agent = sample(AgentTarget::Job("ab12".into()), None);
    agent.id = id.into();
    agent.group = group;
    agent
}

fn at_prompt(id: &str, group: AgentGroup) -> Agent {
    let mut agent = row(id, group);
    agent.at_prompt = true;
    agent
}

#[test]
fn an_idle_session_gets_the_message_at_once() {
    let mut held = Held::default();
    assert_eq!(held.route("a", "one", false, 0), Route::Send);
    assert_eq!(held.get("a"), None);
}

#[test]
fn messages_sent_while_it_works_join_one_held_message() {
    let mut held = Held::default();
    assert_eq!(held.route("a", "one", false, 0), Route::Send);
    assert_eq!(
        held.route("a", "two", true, 1_000),
        Route::Hold("two".into())
    );
    assert_eq!(
        held.route("a", "three", true, 2_000),
        Route::Hold("two\n\nthree".into())
    );
    assert_eq!(held.all().len(), 1);
}

#[test]
fn a_message_right_after_a_send_waits_even_before_the_row_says_working() {
    let mut held = Held::default();
    held.route("a", "one", false, 0);
    assert_eq!(
        held.route("a", "two", false, 500),
        Route::Hold("two".into())
    );
}

#[test]
fn the_held_message_goes_once_the_turn_ends() {
    let mut held = Held::default();
    held.route("a", "one", false, 0);
    held.route("a", "two", true, 1_000);
    held.route("a", "three", true, 1_500);
    assert!(held
        .ready(&[row("a", AgentGroup::Working)], 2_000)
        .is_empty());
    assert_eq!(
        held.ready(&[row("a", AgentGroup::Completed)], 9_000),
        vec![("a".to_string(), "two\n\nthree".to_string())]
    );
    assert_eq!(held.get("a"), None);
    assert_eq!(
        held.route("a", "four", false, 9_500),
        Route::Hold("four".into()),
        "the combined one was just sent, so the next waits for its turn"
    );
}

#[test]
fn it_waits_while_the_session_asks_something() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    assert!(held
        .ready(&[row("a", AgentGroup::NeedsInput)], 1_000)
        .is_empty());
    assert!(held.get("a").is_some());
}

#[test]
fn a_turn_too_quick_to_be_seen_working_still_lets_it_go_after_the_grace() {
    let mut held = Held::default();
    held.route("a", "one", false, 0);
    held.route("a", "two", false, 100);
    assert!(held
        .ready(&[row("a", AgentGroup::Completed)], 1_000)
        .is_empty());
    assert_eq!(
        held.ready(&[row("a", AgentGroup::Completed)], START_GRACE_MS),
        vec![("a".to_string(), "two".to_string())]
    );
}

#[test]
fn a_failed_send_goes_back_in_front_and_waits_before_trying_again() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    let out = held.ready(&[row("a", AgentGroup::Completed)], 1_000);
    held.route("a", "three", true, 1_100);
    held.put_back("a", &out[0].1, 1_200);
    assert_eq!(held.get("a"), Some("two\n\nthree"));
    assert!(held
        .ready(&[row("a", AgentGroup::Completed)], 2_000)
        .is_empty());
}

#[test]
fn a_direct_send_that_failed_does_not_hold_the_next() {
    let mut held = Held::default();
    held.route("a", "one", false, 0);
    held.not_sent("a");
    assert_eq!(held.route("a", "again", false, 100), Route::Send);
}

#[test]
fn held_text_goes_when_the_turn_ended_though_background_tasks_keep_the_row_working() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    assert!(held
        .ready(&[row("a", AgentGroup::Working)], 1_000)
        .is_empty());
    assert_eq!(
        held.ready(&[at_prompt("a", AgentGroup::Working)], 2_000),
        vec![("a".to_string(), "two".to_string())]
    );
}

#[test]
fn held_text_goes_to_a_session_that_ended_its_turn_asking_in_text() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    assert_eq!(
        held.ready(&[at_prompt("a", AgentGroup::NeedsInput)], 1_000),
        vec![("a".to_string(), "two".to_string())]
    );
}

#[test]
fn a_row_working_only_for_background_tasks_does_not_end_the_grace_after_a_send() {
    let mut held = Held::default();
    assert_eq!(held.route("a", "one", false, 0), Route::Send);
    assert!(held
        .ready(&[at_prompt("a", AgentGroup::Working)], 500)
        .is_empty());
    assert_eq!(
        held.route("a", "two", false, 1_000),
        Route::Hold("two".into()),
        "the sent message has not started its turn yet"
    );
    assert!(held
        .ready(&[row("a", AgentGroup::Working)], 1_500)
        .is_empty());
    assert_eq!(
        held.route("a", "three", false, 2_000),
        Route::Hold("two\n\nthree".into())
    );
}

#[test]
fn a_message_that_trails_blank_lines_joins_as_one_paragraph_break() {
    let mut held = Held::default();
    held.route("a", "padding bottom\n\n", true, 0);
    assert_eq!(
        held.route("a", "\n\nhi\n", true, 1_000),
        Route::Hold("padding bottom\n\nhi\n".into())
    );
    assert_eq!(
        held.route("a", "again", true, 2_000),
        Route::Hold("padding bottom\n\nhi\n\nagain".into())
    );
}

#[test]
fn a_single_held_message_keeps_its_own_text() {
    let mut held = Held::default();
    assert_eq!(
        held.route("a", "  fn x() {\n    1\n  }\n", true, 0),
        Route::Hold("  fn x() {\n    1\n  }\n".into())
    );
}
