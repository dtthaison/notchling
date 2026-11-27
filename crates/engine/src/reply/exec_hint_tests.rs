use super::exec::{
    execute, execute_clearing, execute_noting, Clearing, Failure, Markers, NoQueueLog, Outcome, Pty,
};
use super::fake::FakePrompt;
use super::keys::{self, timing::*};
use super::plan;

const HINT: &str = "Try \"refactor parser.rs\"";

fn typed(text: &str) -> Vec<String> {
    vec![
        keys::clear_input(),
        keys::bracketed(text),
        keys::ENTER.to_string(),
    ]
}

fn send(pty: &mut FakePrompt, text: &str) -> Result<Outcome, Failure> {
    execute(&plan::reply(text), pty, &Markers, &mut NoQueueLog)
}

#[test]
fn a_hint_the_session_draws_with_its_own_cursor_is_typed_over_at_once() {
    let mut pty = FakePrompt::idle().hinting(HINT).drawing_its_cursor();
    assert_eq!(send(&mut pty, "hi"), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
    assert_eq!(pty.writes, typed("hi"));
    assert!(pty.now_ms() < INPUT_WAIT_MS, "no wait for a cursor to show");
}

#[test]
fn an_empty_box_the_session_draws_its_own_cursor_in_is_typed_into_at_once() {
    let mut pty = FakePrompt::idle().drawing_its_cursor();
    assert_eq!(send(&mut pty, "hi"), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
    assert!(pty.now_ms() < INPUT_WAIT_MS);
}

#[test]
fn a_message_that_is_the_hint_word_for_word_is_sent_once() {
    let mut pty = FakePrompt::idle().hinting(HINT).drawing_its_cursor();
    assert_eq!(send(&mut pty, HINT), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, [HINT]);
    assert_eq!(pty.writes, typed(HINT));
}

#[test]
fn the_same_words_a_person_typed_are_not_typed_over() {
    let mut pty = FakePrompt::idle().holding(HINT).drawing_its_cursor();
    assert_eq!(send(&mut pty, "hi"), Err(Failure::NotCleared));
    assert!(pty.writes.is_empty(), "no key over a person's words");
    assert_eq!(pty.held(), Some(HINT));
}

#[test]
fn clear_box_and_send_still_clears_the_words_a_person_typed() {
    let mut pty = FakePrompt::idle().holding(HINT).drawing_its_cursor();
    let clearing = Clearing {
        earlier: Vec::new(),
        any: true,
    };
    let out = execute_clearing(
        &plan::reply("hi"),
        &mut pty,
        &Markers,
        &mut NoQueueLog,
        &mut Vec::new(),
        &clearing,
    );
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
}

#[test]
fn the_trail_says_a_hint_was_what_the_box_showed() {
    let mut pty = FakePrompt::idle().hinting(HINT).drawing_its_cursor();
    let mut trail = Vec::new();
    let out = execute_noting(
        &plan::reply("hi"),
        &mut pty,
        &Markers,
        &mut NoQueueLog,
        &mut trail,
    );
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert!(
        trail[0].contains("input box, 1 row, empty, showing a hint, cursor hidden"),
        "{trail:?}"
    );
}

#[test]
fn clear_box_and_send_waits_for_a_box_that_shows_the_hint_again_once_cleared() {
    let mut pty = FakePrompt::idle()
        .holding("my own draft")
        .hinting(HINT)
        .drawing_its_cursor();
    let clearing = Clearing {
        earlier: Vec::new(),
        any: true,
    };
    let out = execute_clearing(
        &plan::reply("hi"),
        &mut pty,
        &Markers,
        &mut NoQueueLog,
        &mut Vec::new(),
        &clearing,
    );
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
    assert_eq!(pty.writes, typed("hi"), "one clear, one paste, one Enter");
}
