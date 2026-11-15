use std::path::PathBuf;

use notchling_engine::outbox::is_settled;
use notchling_locale as t;
use serde_json::{json, Value};

use crate::acting::Request;
use crate::chat::outbox::{Button, OutboxRow, Status};
use crate::tools::requests::Pick;

pub fn status_line(status: &Status) -> String {
    match status {
        Status::Queued => t::chat::outbox_queued().to_owned(),
        Status::QueuedRetry => t::chat::outbox_retry().to_owned(),
        Status::WaitingDialog => t::chat::outbox_dialog().to_owned(),
        Status::WaitsForTurn => t::chat::outbox_turn().to_owned(),
        Status::Sending => t::chat::outbox_sending().to_owned(),
        Status::QueuedInSession => t::chat::outbox_in_queue().to_owned(),
        Status::Sent => t::chat::outbox_sent().to_owned(),
        Status::MayBeIn => t::chat::outbox_maybe().to_owned(),
        Status::BoxHasText(text) => t::chat::outbox_box(&shortened(text)),
        Status::Stopped => t::chat::outbox_stopped().to_owned(),
        Status::NotFound => t::chat::outbox_gone().to_owned(),
    }
}

const SHOWN_MAX: usize = 160;

fn shortened(text: &str) -> String {
    match text.char_indices().nth(SHOWN_MAX) {
        Some((end, _)) => format!("{}…", text[..end].trim_end()),
        None => text.to_owned(),
    }
}

pub fn button_label(button: Button) -> &'static str {
    match button {
        Button::Edit => t::chat::outbox_edit(),
        Button::Remove => t::chat::outbox_remove(),
        Button::SendAgain => t::chat::outbox_send_again(),
        Button::Drop => t::chat::outbox_drop(),
        Button::ClearBoxAndSend => t::chat::outbox_clear_send(),
        Button::ResumeAndSend => t::chat::outbox_resume_send(),
    }
}

pub fn button_id(button: Button) -> &'static str {
    match button {
        Button::Edit => "outbox-edit",
        Button::Remove => "outbox-remove",
        Button::SendAgain => "outbox-send-again",
        Button::Drop => "outbox-drop",
        Button::ClearBoxAndSend => "outbox-clear-send",
        Button::ResumeAndSend => "outbox-resume-send",
    }
}

pub fn is_main(button: Button) -> bool {
    matches!(
        button,
        Button::SendAgain | Button::ClearBoxAndSend | Button::ResumeAndSend
    )
}

pub fn send_request(row: &str, text: &str, pick: Option<&Pick>) -> Request {
    let request = Request::send(row, text);
    match pick.and_then(options_of) {
        Some(options) => request.with_options(options),
        None => request,
    }
}

fn options_of(pick: &Pick) -> Option<Value> {
    let mut options = json!({});
    if let Some(model) = &pick.model {
        options["model"] = json!(model);
    }
    if let Some(effort) = &pick.effort {
        options["effort"] = json!(effort);
    }
    (pick.model.is_some() || pick.effort.is_some()).then_some(options)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Editing {
    pub chat_row: String,
    pub outbox_id: String,
    pub session: String,
    pub before: String,
    pub round: u64,
    pub sending: bool,
}

#[derive(Debug, PartialEq)]
pub struct Begin {
    pub request: Request,
    pub draft_after: Option<String>,
    pub shows_queued: bool,
}

pub fn begin(editing: Option<&Editing>, send: Request, text: &str) -> Begin {
    match editing {
        Some(editing) => Begin {
            request: Request::outbox_edit(&editing.outbox_id, &editing.session, text),
            draft_after: None,
            shows_queued: false,
        },
        None => Begin {
            request: send,
            draft_after: Some(String::new()),
            shows_queued: true,
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct EditEnd {
    pub leave: bool,
    pub draft: Option<String>,
    pub note: Option<String>,
}

impl Editing {
    pub fn finished(&self, result: Result<(), &str>, text: &str) -> EditEnd {
        match result {
            Ok(()) => EditEnd {
                leave: true,
                draft: Some(self.before.clone()),
                note: None,
            },
            Err(message) if is_settled(message) => EditEnd {
                leave: true,
                draft: Some(text.to_owned()),
                note: Some(t::chat::outbox_already_sent().to_owned()),
            },
            Err(message) => EditEnd {
                leave: false,
                draft: None,
                note: Some(message.to_owned()),
            },
        }
    }

    pub fn cancelled(&self) -> String {
        self.before.clone()
    }

    pub fn is_lost(&self, rows: &[OutboxRow]) -> bool {
        !rows.iter().any(|row| row.id == self.outbox_id)
    }

    pub fn lost(&self, chat_row: &str, rows: &[OutboxRow]) -> bool {
        self.chat_row == chat_row && !self.sending && self.is_lost(rows)
    }

    pub fn same_edit(&self, other: &Editing) -> bool {
        self.chat_row == other.chat_row
            && self.outbox_id == other.outbox_id
            && self.round == other.round
    }
}

pub fn settle_edit(current: Option<&Editing>, asked: &Editing, end: EditEnd) -> Option<EditEnd> {
    current.filter(|now| now.same_edit(asked)).map(|_| end)
}

pub fn attachments_for(editing: Option<&Editing>, row: &str, files: Vec<PathBuf>) -> Vec<PathBuf> {
    match editing {
        Some(editing) if editing.chat_row == row => Vec::new(),
        _ => files,
    }
}

#[cfg(test)]
#[path = "outbox_rows_tests.rs"]
mod tests;
