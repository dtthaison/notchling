use super::exec::{execute, Failure, Markers, NoQueueLog, Outcome, Pty};
use super::fake::{FakePrompt, RULE};
use super::keys::{self, timing::*};
use super::plan;

const WITH_PICTURES: &str = "What changed?\n\n/tmp/shots/one.png /tmp/shots/two.jpg";

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
fn a_draft_the_clear_keys_leave_gets_no_message_added() {
    let mut pty = FakePrompt::idle().holding("a long draft").stuck();
    assert_eq!(send(&mut pty, "hi"), Err(Failure::NotCleared));
    assert_eq!(pty.writes, [keys::clear_input()]);
    assert_eq!(pty.held(), Some("a long draft"));
}

#[test]
fn a_stale_draft_is_cleared_before_the_message_goes_in() {
    let mut pty = FakePrompt::new(&["⏺ ok", RULE, "❯ stale", "  draft", RULE]);
    assert_eq!(send(&mut pty, "hi"), Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["hi"]);
}

#[test]
fn the_new_failures_say_what_happened_and_what_to_do() {
    assert_eq!(
        Failure::NoInput.to_string(),
        "The session's terminal isn't at its prompt, so nothing was sent. Open the terminal to see what it shows."
    );
    assert_eq!(
        Failure::NotCleared.to_string(),
        "The session's input box still holds text, so nothing was sent. Clear it in the terminal, then send again."
    );
    assert_eq!(
        Failure::NotTaken.to_string(),
        "The session's terminal didn't take the message, so it wasn't sent. Send it again."
    );
    assert_eq!(
        Failure::NotSent.to_string(),
        "The message is typed in the session's terminal but wasn't sent. Press Return there, or send it again."
    );
}
