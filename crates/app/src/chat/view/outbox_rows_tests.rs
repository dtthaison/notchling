use super::*;
use crate::acting::ops;
use crate::chat::outbox::{bubble, OutboxRow};
use crate::chat::rows::{tail_rows, Row};
use notchling_engine::outbox::{BEING_SENT, IN_QUEUE, NO_MESSAGE};

fn editing() -> Editing {
    Editing {
        chat_row: "job-a".into(),
        outbox_id: "o7".into(),
        before: "a half-typed line".into(),
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
    let begun = begin(None, send_request("job-a", false, "hello", None), "hello");
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
        send_request("job-a", false, "new words", None),
        "new words",
    );
    assert_eq!(begun.request.op, ops::OUTBOX_EDIT);
    assert_eq!(begun.request.args["id"], "o7");
    assert_eq!(begun.request.args["text"], "new words");
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
fn cluster_row_keeps_session_reply() {
    let remote = send_request("job-a", true, "hi", None);
    assert_eq!(remote.op, ops::SESSION_REPLY);
    let local = send_request("job-a", false, "hi", None);
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
    let sent = send_request("codex:t1", false, "go", Some(&pick));
    assert_eq!(
        sent.args["options"],
        json!({"model": "gpt-x", "effort": "high"})
    );
    let none = send_request("codex:t1", false, "go", Some(&Pick::default()));
    assert!(none.args.get("options").is_none());
}

#[test]
fn every_status_has_words_and_every_button_a_label_and_an_id() {
    let words = [
        (Status::Queued, "Queued"),
        (
            Status::QueuedRetry,
            "Queued · couldn't reach the terminal, trying again",
        ),
        (Status::WaitingDialog, "Waiting: dialog open"),
        (Status::WaitsForTurn, "Waits for the current turn"),
        (Status::Sending, "Sending"),
        (Status::QueuedInSession, "Queued in the session"),
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
        (Button::SendAgain, "Send Again"),
        (Button::Drop, "Drop"),
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
