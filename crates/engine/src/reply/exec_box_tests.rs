use super::exec::{execute, execute_noting, Failure, Markers, NoQueueLog, Outcome, Pty};
use super::fake::FakePrompt;
use super::keys::{self, timing::*};
use super::plan;

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
fn text_a_person_types_once_the_box_was_cleared_stops_the_message_before_it_is_pasted() {
    let mut pty = FakePrompt::idle().typing(ATTACH_QUIET_MS + 50, "x");
    assert_eq!(send(&mut pty, "hi"), Err(Failure::NotCleared));
    assert_eq!(pty.writes, [keys::clear_input()], "no paste, no Enter");
    assert!(pty.sent.is_empty());
}

#[test]
fn text_a_person_types_beside_the_paste_is_never_sent_with_it() {
    let mut pty = FakePrompt::idle().typing(ATTACH_QUIET_MS + KEY_PAUSE_MS + 10, "x");
    assert_eq!(send(&mut pty, "hi"), Err(Failure::Mixed));
    assert_eq!(pty.writes, typed("hi")[..2], "no Enter");
    assert!(pty.sent.is_empty());
}

#[test]
fn a_person_typing_before_the_paste_lands_is_not_sent_with_it_either() {
    let mut pty = FakePrompt::idle()
        .pasting_for(400)
        .typing(ATTACH_QUIET_MS + KEY_PAUSE_MS + 100, "my own words");
    assert_eq!(send(&mut pty, "hi"), Err(Failure::Mixed));
    assert!(pty.sent.is_empty());
    assert_eq!(pty.writes.len(), 2, "no Enter");
}

#[test]
fn a_person_typing_after_the_enter_does_not_turn_a_sent_message_into_a_failure() {
    let enter = ATTACH_QUIET_MS + KEY_PAUSE_MS + INPUT_POLL_MS;
    let mut pty = FakePrompt::idle()
        .typing(enter + 10, "ab")
        .typing(enter + 150, "cd")
        .typing(enter + 400, "ef");
    assert_eq!(send(&mut pty, "hi"), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
    assert_eq!(pty.writes, typed("hi"), "one Enter");
}

#[test]
fn a_message_the_session_ignores_is_still_not_sent_with_a_person_typing_beside_it() {
    let enter = ATTACH_QUIET_MS + KEY_PAUSE_MS + INPUT_POLL_MS;
    let mut pty = FakePrompt::idle().deaf().typing(enter + 300, "ab");
    assert_eq!(send(&mut pty, "hi"), Err(Failure::NotSent));
    assert!(pty.sent.is_empty());
}

#[test]
fn a_long_reply_the_session_folds_into_a_placeholder_is_taken_for_the_message() {
    let long = "word ".repeat(300);
    let mut pty = FakePrompt::idle().folding(100);
    assert_eq!(send(&mut pty, &long), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, [long.as_str()]);
}

#[test]
fn a_reply_of_symbols_and_emoji_is_told_by_the_box_not_being_empty() {
    let mut pty = FakePrompt::idle();
    assert_eq!(send(&mut pty, "👍"), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["👍"]);
}

#[test]
fn a_busy_screen_that_never_goes_quiet_does_not_hold_the_attach_for_the_full_wait() {
    let mut pty = FakePrompt::idle().noisy();
    let mut trail = Vec::new();
    let out = execute_noting(
        &plan::reply("hi"),
        &mut pty,
        &Markers,
        &mut NoQueueLog,
        &mut trail,
    );
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
    assert!(
        trail[0].starts_with(&format!("+{ATTACH_BUSY_MS}ms await input")),
        "{trail:?}"
    );
    assert!(pty.now_ms() < ATTACH_TIMEOUT_MS);
}

#[test]
fn a_busy_screen_with_no_prompt_yet_is_still_waited_for_up_to_the_full_wait() {
    let mut pty = FakePrompt::idle().noisy().starting_for(6_000);
    assert_eq!(send(&mut pty, "hi"), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
    assert_eq!(pty.writes, typed("hi"), "nothing typed into the wait");
    assert!(
        pty.now_ms() >= 6_000,
        "the wait lasted until the prompt drew"
    );
}

#[test]
fn a_quiet_screen_ends_the_attach_wait_as_soon_as_it_is_quiet() {
    let mut pty = FakePrompt::idle();
    assert_eq!(send(&mut pty, "hi"), Ok(Outcome { taken: false }));
    let spent = ATTACH_QUIET_MS + KEY_PAUSE_MS + 2 * INPUT_POLL_MS + DETACH_QUIET_MS;
    assert_eq!(pty.now_ms(), spent);
}
