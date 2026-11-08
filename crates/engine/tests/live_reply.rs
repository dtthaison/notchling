//! Live-test mode for E3 (terminal and replies), the Rust twin of `ZLiveReplyTests`. Each test returns at
//! once unless `NOTCHLING_LIVE=1`, because it needs a real Claude Code background session; none is
//! started here, and none of this runs in CI. The bodies are filled in with E3, against a scratch job
//! (a folder named `notchling-record...`) the maintainer starts, as tools/record-reply-fixtures.sh does:
//!
//!     NOTCHLING_LIVE=1 NOTCHLING_LIVE_JOB=<id> cargo test -p notchling-engine --test live_reply
//!
//! Each test says what it will check; until E3 exists they only prove they are switched off or say so.

fn live() -> bool {
    let on = std::env::var("NOTCHLING_LIVE").is_ok_and(|v| v == "1");
    if !on {
        println!("live test off: set NOTCHLING_LIVE=1 (and NOTCHLING_LIVE_JOB for most)");
    }
    on
}

fn job() -> Option<String> {
    std::env::var("NOTCHLING_LIVE_JOB")
        .ok()
        .filter(|v| !v.is_empty())
}

/// Will check: attach, wait for quiet, type `!echo hi` as clear keys + bracketed paste + Enter with the
/// app's pauses, and see the session run it (the transcript gains the `<bash-input>` line).
#[test]
fn live_send_reply() {
    if !live() {
        return;
    }
    assert!(job().is_some(), "NOTCHLING_LIVE_JOB is needed");
    println!("live_send_reply: waiting for E3 (reply typing)");
}

/// Will check: the dialog check refuses a reply while a question or permission dialog is open
/// (`NOTCHLING_LIVE_ASK_JOB`), and types nothing into it.
#[test]
fn live_refuses_reply_into_a_dialog() {
    if !live() {
        return;
    }
    assert!(
        std::env::var("NOTCHLING_LIVE_ASK_JOB").is_ok(),
        "NOTCHLING_LIVE_ASK_JOB is needed"
    );
    println!("live_refuses_reply_into_a_dialog: waiting for E3 (dialog check)");
}

/// Will check: the open AskUserQuestion is read with Right/Left over its tabs and every question,
/// option and multi-select flag comes back; with `NOTCHLING_LIVE_ANSWER=1` it is also answered, ending on
/// the review screen with "1".
#[test]
fn live_reads_and_answers_a_question() {
    if !live() {
        return;
    }
    assert!(
        std::env::var("NOTCHLING_LIVE_ASK_JOB").is_ok(),
        "NOTCHLING_LIVE_ASK_JOB is needed"
    );
    println!("live_reads_and_answers_a_question: waiting for E3 (question reading)");
}

/// Will check: a terminal started in the pty host draws `/plugin`'s menu, redraws for a smaller size
/// after a resize (the forkpty behavior), and Esc closes the menu. No answer is sent to device-attribute
/// queries, and an inherited `NO_COLOR` is dropped.
#[test]
fn live_terminal_menu_and_resize() {
    if !live() {
        return;
    }
    assert!(job().is_some(), "NOTCHLING_LIVE_JOB is needed");
    println!("live_terminal_menu_and_resize: waiting for E3 (pty host)");
}

/// Will check: Esc stops a working session's turn, and is refused for an idle one.
#[test]
fn live_interrupt_only_while_working() {
    if !live() {
        return;
    }
    assert!(job().is_some(), "NOTCHLING_LIVE_JOB is needed");
    println!("live_interrupt_only_while_working: waiting for E3 (interrupt)");
}

/// Will check: a reply sent while the session works is queued; taking it back (clear, Up, clear) is
/// confirmed by a `popAll` line in the transcript, and the older queued messages go back in order.
#[test]
fn live_take_back_queued() {
    if !live() {
        return;
    }
    assert!(job().is_some(), "NOTCHLING_LIVE_JOB is needed");
    assert!(
        std::env::var("NOTCHLING_LIVE_TRANSCRIPT").is_ok(),
        "NOTCHLING_LIVE_TRANSCRIPT is needed"
    );
    println!("live_take_back_queued: waiting for E3 (queue)");
}
