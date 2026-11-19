use super::*;
use crate::acting::ops;
use crate::chat::outbox::{bubble, OutboxRow};
use crate::chat::rows::{tail_rows, Row};
use notchling_engine::outbox::{BEING_SENT, CHANGED, IN_QUEUE, NO_MESSAGE};

fn editing() -> Editing {
    Editing {
        chat_row: "job-a".into(),
        outbox_id: "o7".into(),
        session: "ab12cd34".into(),
        before: "a half-typed line".into(),
        was: "the queued text".into(),
        round: 1,
        sending: false,
    }
}

fn queued_row(text: &str) -> OutboxRow {
    OutboxRow::from_wire(&json!({
        "id": "o1", "session": "ab12cd34", "tool": "claude", "kind": "prompt", "text": text,
        "state": "queued", "position": 1, "queuedMs": 1, "updatedMs": 1, "via": "chat",
    }))
    .expect("a row")
}

#[test]
fn send_returns_and_clears_box_at_once() {
    let begun = begin(None, send_request("job-a", "hello", None), "hello");
    assert_eq!(begun.draft_after.as_deref(), Some(""));
    assert!(begun.shows_queued);
    let rows = tail_rows(&[], None, &[], &["hello".to_owned()], begun.shows_queued);
    let Row::User {
        outbox, sending, ..
    } = &rows[0]
    else {
        panic!("a user row");
    };
    assert!(!sending);
    let shown = outbox.as_ref().expect("a bubble before any delivery");
    assert_eq!(status_line(&shown.status), "Queued");
    assert!(shown.buttons.is_empty());
}

#[test]
fn edit_mode_sends_outbox_edit() {
    let editing = editing();
    let begun = begin(
        Some(&editing),
        send_request("job-a", "new words", None),
        "new words",
    );
    assert_eq!(begun.request.op, ops::OUTBOX_EDIT);
    assert_eq!(begun.request.args["id"], "o7");
    assert_eq!(begun.request.args["session"], "ab12cd34");
    assert_eq!(begun.request.args["text"], "new words");
    assert_eq!(begun.request.args["from"], "the queued text");
    assert_eq!(begun.draft_after, None);
    assert!(!begun.shows_queued);
    let end = editing.finished(Ok(()), "new words");
    assert!(end.leave);
    assert_eq!(end.draft.as_deref(), Some("a half-typed line"));
    assert_eq!(end.note, None);
    assert_eq!(editing.cancelled(), "a half-typed line");
}

#[test]
fn an_edit_of_a_message_that_is_gone_or_sent_keeps_the_text_as_a_draft() {
    for message in [NO_MESSAGE, BEING_SENT, IN_QUEUE] {
        let end = editing().finished(Err(message), "new words");
        assert!(end.leave, "{message}");
        assert_eq!(end.draft.as_deref(), Some("new words"), "{message}");
        assert_eq!(end.note.as_deref(), Some("Already sent"), "{message}");
    }
}

#[test]
fn any_other_refusal_keeps_the_edit_open_with_the_daemons_words() {
    for message in [
        "Type a message first.",
        "That message is over 100 KB.",
        "That command opens a menu; open the session's terminal.",
        "Couldn't save the message.",
        "Notchling's daemon isn't running.",
    ] {
        let end = editing().finished(Err(message), "new words");
        assert!(!end.leave, "{message}");
        assert_eq!(end.draft, None, "{message}");
        assert_eq!(end.note.as_deref(), Some(message));
    }
}

#[test]
fn an_edit_is_lost_when_its_row_is_no_longer_in_the_outbox() {
    let editing = editing();
    assert!(editing.is_lost(&[]));
    assert!(editing.is_lost(&[queued_row("x")]));
    let mut here = queued_row("x");
    here.id = "o7".into();
    assert!(!editing.is_lost(&[queued_row("y"), here]));
}

#[test]
fn an_answer_ends_only_the_edit_that_asked() {
    let asked = editing();
    let ends = || asked.finished(Ok(()), "new words");
    assert_eq!(settle_edit(Some(&asked), &asked, ends()), Some(ends()));
    assert_eq!(settle_edit(None, &asked, ends()), None);
    let began_again = Editing {
        round: 2,
        ..editing()
    };
    assert_eq!(settle_edit(Some(&began_again), &asked, ends()), None);
    let another_row = Editing {
        outbox_id: "o8".into(),
        ..editing()
    };
    assert_eq!(settle_edit(Some(&another_row), &asked, ends()), None);
    let another_chat = Editing {
        chat_row: "job-b".into(),
        ..editing()
    };
    assert_eq!(settle_edit(Some(&another_chat), &asked, ends()), None);
}

#[test]
fn an_edit_whose_request_is_in_flight_is_not_lost() {
    let mut asked = editing();
    assert!(asked.lost("job-a", &[]));
    assert!(!asked.lost("job-b", &[]));
    asked.sending = true;
    assert!(!asked.lost("job-a", &[]));
    assert!(asked.is_lost(&[]));
}

#[test]
fn edit_mode_attaches_nothing_to_the_message() {
    let files = vec![std::path::PathBuf::from("/tmp/a.png")];
    let editing = editing();
    assert!(attachments_for(Some(&editing), "job-a", files.clone()).is_empty());
    assert_eq!(
        attachments_for(Some(&editing), "job-b", files.clone()),
        files
    );
    assert_eq!(attachments_for(None, "job-a", files.clone()), files);
}

#[test]
fn cluster_row_sends_through_the_outbox_like_a_local_one() {
    let remote = send_request("n1/job-a", "hi", None);
    assert_eq!(remote.op, ops::SESSION_SEND);
    assert_eq!(remote.args["id"], "n1/job-a");
    let local = send_request("job-a", "hi", None);
    assert_eq!(local.op, ops::SESSION_SEND);
    assert_eq!(local.args["id"], "job-a");
    assert!(local.args.get("options").is_none());
}

#[test]
fn a_codex_pick_rides_as_options_and_an_empty_one_does_not() {
    let pick = Pick {
        model: Some("gpt-x".into()),
        effort: Some("high".into()),
    };
    let sent = send_request("codex:t1", "go", Some(&pick));
    assert_eq!(
        sent.args["options"],
        json!({"model": "gpt-x", "effort": "high"})
    );
    let none = send_request("codex:t1", "go", Some(&Pick::default()));
    assert!(none.args.get("options").is_none());
}

#[test]
fn every_status_has_words_and_every_button_a_label_and_an_id() {
    let words = [
        (Status::Queued, "Queued"),
        (
            Status::QueuedRetry,
            "Couldn't reach the session, trying again",
        ),
        (Status::WaitingDialog, "Waiting for a dialog"),
        (Status::WaitsForTurn, "Waiting for the turn to end"),
        (
            Status::WaitsForQueue(Tool::Claude),
            "Waiting for Claude Code to read the message before",
        ),
        (
            Status::WaitsForQueue(Tool::Codex),
            "Waiting for Codex to read the message before",
        ),
        (Status::WaitsForPrevious, "Waiting for the message before"),
        (Status::Editing, "Editing"),
        (Status::Sending, "Sending…"),
        (
            Status::QueuedInSession(Tool::Claude),
            "In Claude Code's queue",
        ),
        (
            Status::QueuedInSession(Tool::Antigravity),
            "In Antigravity's queue",
        ),
        (Status::Sent, "Sent"),
        (Status::MayBeIn, "May already be in the session"),
        (
            Status::BoxHasText("other words".into()),
            "Box has other text: other words",
        ),
        (Status::Stopped, "Session stopped"),
        (Status::NotFound, "Session not found"),
    ];
    for (status, want) in words {
        assert_eq!(status_line(&status), want);
    }
    let labels = [
        (Button::Edit, "Edit"),
        (Button::Remove, "Remove"),
        (Button::Retry, "Retry"),
        (Button::MarkAsSent, "Mark as Sent"),
        (Button::ClearBoxAndSend, "Clear Box and Send"),
        (Button::ResumeAndSend, "Resume and Send"),
    ];
    let mut ids = std::collections::HashSet::new();
    for (button, want) in labels {
        assert_eq!(button_label(button), want);
        assert!(ids.insert(button_id(button)));
    }
}

#[test]
fn a_long_box_text_is_cut_to_one_short_line() {
    let long = "w ".repeat(400);
    let line = status_line(&Status::BoxHasText(long.trim().to_owned()));
    assert!(line.ends_with('…'));
    assert!(line.chars().count() < 200);
    let row = queued_row("x");
    assert_eq!(bubble(&row).status, Status::Queued);
}

#[test]
fn every_render_state_makes_its_bubbles() {
    for state in crate::chat::renders::OUTBOX_STATES {
        let shown = crate::chat::renders::outbox_preview(state).unwrap_or_default();
        assert!(!shown.is_empty(), "{state}");
    }
    assert!(crate::chat::renders::outbox_preview("nothing").is_none());
    let three = crate::chat::renders::outbox_preview("three").unwrap_or_default();
    assert_eq!(three.len(), 3);
}

#[test]
fn an_edit_refused_because_the_message_changed_stays_open_and_asks_for_the_current_text() {
    let end = editing().finished(Err(CHANGED), "my words");
    assert!(!end.leave);
    assert_eq!(end.draft, None);
    assert!(end.refresh);
    let note = end.note.expect("a note");
    assert!(note.contains("changed"), "{note}");
    for end in [
        editing().finished(Ok(()), "x"),
        editing().finished(Err("boom"), "x"),
        editing().finished(Err(NO_MESSAGE), "x"),
    ] {
        assert!(!end.refresh);
    }
}

#[test]
fn the_current_text_of_a_changed_row_is_read_from_the_feed() {
    let mut joined = queued_row("the queued text\n\nand more");
    joined.id = "o7".into();
    let rows = [queued_row("other"), joined];
    assert_eq!(
        editing().current_text(&rows),
        Some("the queued text\n\nand more")
    );
    let unchanged = {
        let mut row = queued_row("the queued text");
        row.id = "o7".into();
        [row]
    };
    assert_eq!(editing().current_text(&unchanged), None);
    assert_eq!(editing().current_text(&[queued_row("x")]), None);
}

#[test]
fn an_open_edit_renews_its_hold_well_before_the_lease_runs_out() {
    let open = editing();
    const { assert!(HOLD_RENEW_MS * 2 <= HOLD_LEASE_MS) };
    assert!(!hold_due(Some(&open), 1_000, 1_000 + HOLD_RENEW_MS - 1));
    assert!(hold_due(Some(&open), 1_000, 1_000 + HOLD_RENEW_MS));
    assert!(!hold_due(None, 0, i64::MAX));
    let saving = Editing {
        sending: true,
        ..editing()
    };
    assert!(!hold_due(Some(&saving), 0, i64::MAX));
}

#[test]
fn a_refused_hold_ends_the_edit_and_says_why() {
    let open = editing();
    for message in [NO_MESSAGE, BEING_SENT, IN_QUEUE] {
        let end = open.hold_refused(message, "my words");
        assert!(end.leave, "{message}");
        assert_eq!(end.draft.as_deref(), Some("my words"));
        assert_eq!(end.note.as_deref(), Some(t::chat::outbox_already_sent()));
    }
    let end = open.hold_refused("Couldn't reach Notchling.", "my words");
    assert!(end.leave);
    assert_eq!(end.draft.as_deref(), Some("a half-typed line"));
    assert_eq!(end.note.as_deref(), Some("Couldn't reach Notchling."));
    assert!(!end.refresh);
}
