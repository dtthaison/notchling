use std::path::PathBuf;

use gpui::{Div, Styled};
use notchling_kit::ui::icon;
use notchling_kit::{Appearance, Icon, Ink};

use notchling_engine::outbox::{is_settled, Tool, CHANGED};
use notchling_locale as t;
use serde_json::{json, Value};

use crate::acting::Request;
use crate::chat::outbox::{Bubble, Button, OutboxRow, Status};
use crate::tools::requests::Pick;

fn tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Claude => t::chat::outbox_tool_claude(),
        Tool::Codex => t::chat::outbox_tool_codex(),
        Tool::Antigravity => t::chat::outbox_tool_antigravity(),
    }
}

pub fn status_icon(status: &Status, a: Appearance) -> Div {
    let (glyph, ink) = match status {
        Status::Editing => (Icon::Edit, Ink::TextFaint),
        Status::MayBeIn
        | Status::MayBeInHolding(_)
        | Status::BoxHasText(_)
        | Status::BoxHoldsThis => (Icon::Warning, Ink::Needs),
        Status::Stopped => (Icon::Stop, Ink::TextFaint),
        Status::NotFound => (Icon::Fail, Ink::Needs),
        _ => (Icon::Clock, Ink::TextFaint),
    };
    icon(glyph, 13.0, ink, a).flex_shrink_0()
}

pub fn countdown(until_ms: i64, now_ms: i64) -> String {
    let left = ((until_ms - now_ms).max(0) + 999) / 1000;
    format!("{}:{:02}", left / 60, left % 60)
}

pub fn bubble_line(bubble: &Bubble, now_ms: i64) -> String {
    let line = status_line_at(&bubble.status, Some(now_ms));
    if bubble.joined > 1 {
        format!(
            "{} · {line}",
            t::chat::outbox_joined(i64::from(bubble.joined))
        )
    } else {
        line
    }
}

#[cfg(test)]
pub fn status_line(status: &Status) -> String {
    status_line_at(status, None)
}

pub fn status_line_at(status: &Status, now_ms: Option<i64>) -> String {
    match status {
        Status::Queued => t::chat::outbox_queued().to_owned(),
        Status::QueuedRetry => t::chat::outbox_retry().to_owned(),
        Status::WaitingDialog => t::chat::outbox_dialog().to_owned(),
        Status::WaitsForTurn => t::chat::outbox_turn().to_owned(),
        Status::WaitsForQueue(tool) => t::chat::outbox_wait_queue(tool_name(*tool)),
        Status::WaitsForPrevious => t::chat::outbox_wait_unsure().to_owned(),
        Status::Editing => t::chat::outbox_editing_row().to_owned(),
        Status::Sending => t::chat::outbox_sending().to_owned(),
        Status::QueuedInSession(tool) => t::chat::outbox_in_queue(tool_name(*tool)),
        Status::Sent => t::chat::outbox_sent().to_owned(),
        Status::MayBeIn => t::chat::outbox_maybe().to_owned(),
        Status::MayBeInHolding(until) => match now_ms {
            Some(now) => format!(
                "{} · {}",
                t::chat::outbox_maybe(),
                t::chat::outbox_hold_left(&countdown(*until, now))
            ),
            None => t::chat::outbox_maybe().to_owned(),
        },
        Status::BoxHasText(text) => t::chat::outbox_box(&shortened(text)),
        Status::BoxHoldsThis => t::chat::outbox_box_same().to_owned(),
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
        Button::Retry => t::chat::outbox_retry_send(),
        Button::MarkAsSent => t::chat::outbox_mark_sent(),
        Button::ClearBoxAndSend => t::chat::outbox_clear_send(),
        Button::ResumeAndSend => t::chat::outbox_resume_send(),
    }
}

pub fn button_id(button: Button) -> &'static str {
    match button {
        Button::Edit => "outbox-edit",
        Button::Remove => "outbox-remove",
        Button::Retry => "outbox-retry",
        Button::MarkAsSent => "outbox-mark-sent",
        Button::ClearBoxAndSend => "outbox-clear-send",
        Button::ResumeAndSend => "outbox-resume-send",
    }
}

pub fn is_main(button: Button) -> bool {
    matches!(
        button,
        Button::Retry | Button::ClearBoxAndSend | Button::ResumeAndSend
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
    pub was: String,
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
            request: Request::outbox_edit(&editing.outbox_id, &editing.session, text, &editing.was),
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
    pub refresh: bool,
}

impl Editing {
    pub fn finished(&self, result: Result<(), &str>, text: &str) -> EditEnd {
        match result {
            Ok(()) => EditEnd {
                leave: true,
                draft: Some(self.before.clone()),
                note: None,
                refresh: false,
            },
            Err(CHANGED) => EditEnd {
                leave: false,
                draft: None,
                note: Some(t::chat::outbox_changed().to_owned()),
                refresh: true,
            },
            Err(message) if is_settled(message) => EditEnd {
                leave: true,
                draft: Some(text.to_owned()),
                note: Some(t::chat::outbox_already_sent().to_owned()),
                refresh: false,
            },
            Err(message) => EditEnd {
                leave: false,
                draft: None,
                note: Some(message.to_owned()),
                refresh: false,
            },
        }
    }

    pub fn current_text<'a>(&self, rows: &'a [OutboxRow]) -> Option<&'a str> {
        rows.iter()
            .find(|row| row.id == self.outbox_id)
            .map(|row| row.text.as_str())
            .filter(|text| *text != self.was)
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

#[cfg(test)]
#[path = "outbox_box_same_tests.rs"]
mod box_same_tests;

#[cfg(test)]
#[path = "outbox_queue_tests.rs"]
mod queue_tests;
