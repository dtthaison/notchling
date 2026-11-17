use super::prompt_flow::failure_text;
use crate::acting::failure_message;
use notchling_local_api::ClientError;
use std::io;

#[test]
fn an_answer_that_got_no_reply_because_the_mac_is_busy_says_it_may_have_gone_through() {
    let slow = [
        ClientError::Timeout,
        ClientError::Io(io::Error::from(io::ErrorKind::WouldBlock)),
        ClientError::Io(io::Error::from(io::ErrorKind::TimedOut)),
    ];
    for error in slow {
        let label = error.to_string();
        let shown = failure_text(&failure_message(error));
        assert!(shown.starts_with("Notchling is busy."), "{label}: {shown}");
        assert!(shown.contains("may still have reached"), "{label}: {shown}");
        assert!(!shown.contains("Try again"), "{label}: {shown}");
    }
}

#[test]
fn an_answer_the_daemon_refused_is_shown_as_it_said() {
    let said = "The terminal asks about something else than Chat shows, so nothing was typed. Answer it in the terminal.";
    assert_eq!(failure_text(said), said);
}
