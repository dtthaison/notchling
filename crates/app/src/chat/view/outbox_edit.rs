use gpui::Context;
use notchling_engine::outbox::NO_MESSAGE;
use notchling_engine::sessions::Agent;
use notchling_locale as t;

use super::outbox_rows::{settle_edit, EditEnd, Editing};
use super::ChatView;
use crate::acting::{Acting, Request};
use crate::chat::outbox::Button;

impl ChatView {
    pub(super) fn outbox_press(&mut self, id: &str, button: Button, cx: &mut Context<Self>) {
        if button == Button::Edit {
            self.begin_edit(id, cx);
            return;
        }
        let Some(session) = self.selected_session().map(|s| s.agent.outbox_key()) else {
            return;
        };
        let Some(request) = button.request(id, &session) else {
            return;
        };
        if self
            .tools
            .editing
            .as_ref()
            .is_some_and(|e| e.outbox_id == id)
        {
            self.cancel_edit(cx);
        }
        if matches!(button, Button::Remove | Button::Drop) {
            self.tools.forget_outbox(id);
        }
        self.spawn_send(
            request,
            |view, result, _| {
                if let Err(message) = result {
                    view.tools.error = Some(message);
                }
            },
            cx,
        );
    }

    fn begin_edit(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(session) = self.selected_session().map(|s| s.agent.clone()) else {
            return;
        };
        let Some(row) = self
            .source
            .outbox_rows(&session)
            .into_iter()
            .find(|row| row.id == id)
        else {
            return;
        };
        let chat_row = session.id.clone();
        let before = match &self.tools.editing {
            Some(editing) if editing.chat_row == chat_row => editing.before.clone(),
            _ => self.tools.draft(&chat_row).to_owned(),
        };
        self.tools.edit_round += 1;
        self.tools.editing = Some(Editing {
            chat_row: chat_row.clone(),
            outbox_id: row.id,
            session: row.session,
            before,
            was: row.text.clone(),
            round: self.tools.edit_round,
            sending: false,
        });
        self.put_draft(&chat_row, row.text, cx);
        cx.notify();
    }

    pub(super) fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.tools.editing.take() else {
            return;
        };
        self.put_draft(&editing.chat_row, editing.cancelled(), cx);
        cx.notify();
    }

    pub(super) fn editing_here(&self) -> bool {
        self.tools
            .editing
            .as_ref()
            .is_some_and(|e| self.selected.as_deref() == Some(e.chat_row.as_str()))
    }

    pub(super) fn submit_edit(
        &mut self,
        editing: Editing,
        request: Request,
        text: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = self.tools.editing.as_mut() {
            open.sending = true;
        }
        self.spawn_send(
            request,
            move |view, result, cx| {
                let end =
                    editing.finished(result.as_ref().map(|_| ()).map_err(String::as_str), &text);
                view.end_edit(&editing, end, cx);
            },
            cx,
        );
    }

    fn end_edit(&mut self, editing: &Editing, end: EditEnd, cx: &mut Context<Self>) {
        let Some(end) = settle_edit(self.tools.editing.as_ref(), editing, end) else {
            return;
        };
        if end.leave {
            self.tools.editing = None;
            self.tools.forget_outbox(&editing.outbox_id);
        } else if let Some(open) = self.tools.editing.as_mut() {
            open.sending = false;
        }
        let shown = end.refresh && self.show_current_text(editing, cx);
        if let Some(draft) = end.draft {
            self.put_draft(&editing.chat_row, draft, cx);
        }
        self.tools.error = if shown {
            Some(t::chat::outbox_changed_shown().to_owned())
        } else {
            end.note
        };
    }

    fn show_current_text(&mut self, editing: &Editing, cx: &mut Context<Self>) -> bool {
        let Some(agent) = self
            .selected_session()
            .map(|s| s.agent.clone())
            .filter(|agent| agent.id == editing.chat_row)
        else {
            return false;
        };
        let rows = self.source.outbox_rows(&agent);
        let Some(current) = editing.current_text(&rows).map(str::to_owned) else {
            return false;
        };
        if let Some(open) = self.tools.editing.as_mut() {
            open.was = current.clone();
        }
        self.put_draft(&editing.chat_row, current, cx);
        true
    }

    pub(super) fn leave_lost_edit(&mut self, agent: &Agent, cx: &mut Context<Self>) {
        if self.acting(cx) != Acting::On {
            return;
        }
        let rows = self.source.outbox_rows(agent);
        let Some(editing) = self
            .tools
            .editing
            .clone()
            .filter(|e| e.lost(&agent.id, &rows))
        else {
            return;
        };
        let typed = self.tools.editor.read(cx).text().to_owned();
        let end = editing.finished(Err(NO_MESSAGE), &typed);
        self.end_edit(&editing, end, cx);
    }
}
