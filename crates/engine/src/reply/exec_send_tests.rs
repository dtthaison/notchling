use super::exec::{execute, execute_noting, Failure, Markers, NoQueueLog, Outcome, Pty};
use super::fake::{FakePrompt, RULE, STARTING};
use super::keys::{self, timing::*};
use super::plan;

const WITH_PICTURES: &str = "What changed?\n\n/tmp/shots/one.png /tmp/shots/two.jpg";
const WITH_SHOT: &str = "i want click refresh it will refresh is it ok?\n\n'/Users/me/Library/Application Support/Notchling/drops/1794723941465/Screenshot 2026-11-15 at 1.25.35 PM.png'";

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
fn a_paste_the_session_takes_late_is_still_sent() {
    let mut pty = FakePrompt::idle().pasting_for(400);
    let out = execute(
        &plan::reply(WITH_PICTURES),
        &mut pty,
        &Markers,
        &mut NoQueueLog,
    );
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, [WITH_PICTURES], "an ok means the session took it");
    assert_eq!(pty.held(), Some(""), "nothing is left in the box");
    assert_eq!(pty.writes, typed(WITH_PICTURES));
}

#[test]
fn a_plain_reply_is_sent_with_one_poll_of_waiting_each_side_of_enter() {
    let mut pty = FakePrompt::idle();
    assert_eq!(send(&mut pty, "hi"), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
    assert_eq!(pty.writes, typed("hi"));
    let spent = ATTACH_QUIET_MS + KEY_PAUSE_MS + 2 * INPUT_POLL_MS + DETACH_QUIET_MS;
    assert_eq!(pty.now_ms(), spent);
}

#[test]
fn a_new_sessions_hint_in_the_box_is_not_taken_for_the_message() {
    let mut pty = FakePrompt::idle()
        .hinting("Try \"how do I log an error?\"")
        .pasting_for(700);
    assert_eq!(send(&mut pty, WITH_PICTURES), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, [WITH_PICTURES]);
}

#[test]
fn a_session_that_is_still_starting_gets_no_key_until_its_own_prompt_is_up() {
    let mut pty = FakePrompt::idle().starting_for(2_000);
    assert_eq!(send(&mut pty, "hi"), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
    assert_eq!(pty.writes, typed("hi"), "nothing was typed into the wait");
}

#[test]
fn a_start_longer_than_the_wait_for_a_prompt_is_waited_out() {
    let mut pty = FakePrompt::idle().starting_for(INPUT_WAIT_MS + 7_000);
    assert_eq!(send(&mut pty, "hi"), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
    assert_eq!(pty.writes, typed("hi"));
}

#[test]
fn a_short_wait_in_other_words_is_told_by_the_cursor_missing_from_its_box() {
    let mut pty = FakePrompt::idle()
        .starting_for(2_000)
        .noting("  Loading the session");
    assert_eq!(send(&mut pty, "hi"), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
    assert_eq!(pty.writes, typed("hi"));
}

#[test]
fn a_prompt_that_came_up_late_is_typed_into_only_once_it_has_stood_still() {
    let mut pty = FakePrompt::idle().starting_for(2_000);
    let mut trail = Vec::new();
    let out = execute_noting(
        &plan::reply("hi"),
        &mut pty,
        &Markers,
        &mut NoQueueLog,
        &mut trail,
    );
    assert_eq!(out, Ok(Outcome { taken: false }));
    let prompt = "input box, 1 row, empty, cursor at 1,2 in the box, under it: ? for shortcuts";
    assert_eq!(
        trail,
        [
            format!("+500ms await input: starting: {}", STARTING.trim()),
            format!("+2000ms await input: {prompt}"),
            format!("+{}ms await empty: {prompt}", 2_000 + START_SETTLE_MS + KEY_PAUSE_MS),
            format!("+{}ms await paste: input box, 1 row, holding text, cursor at 1,4 in the box, under it: ? for shortcuts", 2_000 + START_SETTLE_MS + KEY_PAUSE_MS),
            format!("+{}ms submit: input box, 1 row, holding text, cursor at 1,4 in the box, under it: ? for shortcuts", 2_000 + START_SETTLE_MS + KEY_PAUSE_MS + INPUT_POLL_MS),
            format!("+{}ms submit: {prompt}", 2_000 + START_SETTLE_MS + KEY_PAUSE_MS + INPUT_POLL_MS),
        ]
    );
}

#[test]
fn a_message_the_session_never_takes_is_an_error_and_gets_no_enter() {
    let mut pty = FakePrompt::idle().pasting_for(PASTE_WAIT_MS + 5_000);
    assert_eq!(send(&mut pty, WITH_PICTURES), Err(Failure::NotTaken));
    assert!(pty.sent.is_empty());
    assert_eq!(pty.writes, typed(WITH_PICTURES)[..2]);
}

#[test]
fn a_message_that_stays_in_the_box_after_enter_is_an_error() {
    let mut pty = FakePrompt::idle().deaf();
    assert_eq!(send(&mut pty, "hi"), Err(Failure::NotSent));
    assert!(pty.sent.is_empty());
    assert_eq!(pty.held(), Some("hi"));
    assert_eq!(pty.writes, typed("hi"), "one Enter, never a second");
}

#[test]
fn a_screen_with_no_input_box_gets_no_keys() {
    let mut pty = FakePrompt::new(&["Resume a session", "  1. fix-hook", "  2. add-tests"]);
    assert_eq!(send(&mut pty, "hi"), Err(Failure::NoInput));
    assert!(pty.writes.is_empty());
    assert!(
        pty.now_ms() >= INPUT_WAIT_MS,
        "it waited for the box to draw"
    );
}

#[test]
fn a_draft_in_the_box_is_never_typed_over() {
    let mut pty = FakePrompt::idle().holding("a long draft");
    assert_eq!(send(&mut pty, "hi"), Err(Failure::NotCleared));
    assert!(pty.writes.is_empty(), "not a key, not even the clear");
    assert_eq!(pty.held(), Some("a long draft"));
}

#[test]
fn a_stale_draft_on_the_screen_stops_the_message_too() {
    let mut pty = FakePrompt::new(&["⏺ ok", RULE, "❯ stale", "  draft", RULE]);
    assert_eq!(send(&mut pty, "hi"), Err(Failure::NotCleared));
    assert!(pty.writes.is_empty());
    assert!(pty.sent.is_empty());
}

#[test]
fn the_message_itself_left_in_the_box_by_an_earlier_try_is_cleared_and_sent_once() {
    let mut pty = FakePrompt::idle().holding("hi");
    assert_eq!(send(&mut pty, "hi"), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
    assert_eq!(pty.writes, typed("hi"));
}

#[test]
fn the_new_failures_say_what_happened_and_what_to_do() {
    assert_eq!(
        Failure::NoInput.to_string(),
        "The session's terminal isn't at its prompt, so nothing was sent. Open the terminal to see what it shows."
    );
    assert_eq!(
        Failure::NotCleared.to_string(),
        "The session's input box holds other text, so the message wasn't sent."
    );
    assert_eq!(
        Failure::NotTaken.to_string(),
        "The session's terminal didn't take the message, so it wasn't sent. Send it again."
    );
    assert_eq!(
        Failure::Mixed.to_string(),
        "Something else was typed in the session's input box while the message went in, so it wasn't sent. Check the terminal."
    );
    assert_eq!(
        Failure::NotSent.to_string(),
        "The message is typed in the session's terminal but wasn't sent. Press Return there, or send it again."
    );
}

#[test]
fn a_message_whose_pictures_the_session_turned_into_chips_is_sent_once() {
    let mut pty = FakePrompt::idle().reading_pictures().pasting_for(400);
    assert_eq!(send(&mut pty, WITH_SHOT), Ok(Outcome { taken: false }));
    assert_eq!(
        pty.sent,
        ["[Image #1]i want click refresh it will refresh is it ok?"]
    );
    assert_eq!(
        pty.writes,
        typed(WITH_SHOT),
        "one clear, one paste, one Enter"
    );
    assert_eq!(pty.held(), Some(""));

    let mut pty = FakePrompt::idle().reading_pictures();
    assert_eq!(send(&mut pty, WITH_PICTURES), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["[Image #1] [Image #2]What changed?"]);
    assert_eq!(pty.writes, typed(WITH_PICTURES));
}

#[test]
fn a_chipped_message_the_session_ignores_is_not_sent_and_gets_one_enter() {
    let mut pty = FakePrompt::idle().reading_pictures().deaf();
    assert_eq!(send(&mut pty, WITH_PICTURES), Err(Failure::NotSent));
    assert!(pty.sent.is_empty());
    assert_eq!(
        pty.writes,
        typed(WITH_PICTURES),
        "one Enter, never a second"
    );
}

#[test]
fn text_a_person_types_beside_the_chips_is_never_sent_with_them() {
    let mut pty = FakePrompt::idle()
        .reading_pictures()
        .typing(ATTACH_QUIET_MS + KEY_PAUSE_MS + 10, "my own words");
    assert_eq!(send(&mut pty, WITH_PICTURES), Err(Failure::Mixed));
    assert!(pty.sent.is_empty());
    assert_eq!(pty.writes, typed(WITH_PICTURES)[..2], "no Enter");
}

#[test]
fn a_message_that_names_a_chip_in_words_is_sent_once_and_a_foreign_chip_beside_it_is_not() {
    let text = "why does [Image #1] fail?";
    let mut pty = FakePrompt::idle().reading_pictures();
    assert_eq!(send(&mut pty, text), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, [text]);
    assert_eq!(pty.writes, typed(text));

    let mut pty = FakePrompt::idle()
        .reading_pictures()
        .typing(ATTACH_QUIET_MS + KEY_PAUSE_MS + 10, "[Image #9]");
    assert_eq!(send(&mut pty, text), Err(Failure::Mixed));
    assert!(pty.sent.is_empty());
    assert_eq!(pty.writes, typed(text)[..2], "no Enter");
}
