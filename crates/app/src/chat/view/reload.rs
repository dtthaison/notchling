use gpui::{AppContext, ClipboardItem, Context};
use serde_json::Value;

use super::ChatView;
use crate::acting::Request;
use crate::chat::reload::{self, Act, Applied, Card, Stage};
use crate::sheets::reload_view::{ReloadChoice, ReloadView};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sent {
    Start,
    Poll,
}

impl ChatView {
    pub(super) fn reload_mode(&self, id: &str) -> Option<reload::Mode> {
        self.sessions
            .iter()
            .find(|s| s.agent.id == id)
            .and_then(reload::mode)
    }

    pub(super) fn reload_offered(&self, id: &str) -> bool {
        self.reload_mode(id).is_some()
    }

    pub(super) fn reload_note(&self) -> Option<String> {
        let id = self.selected.as_deref()?;
        self.reload.note(id, super::now_ms())
    }

    fn reload_lead(&self, id: &str) -> String {
        let facts = &self.conversation.facts;
        let known = self.selected.as_deref() == Some(id);
        reload::lead(
            if known { facts.context_tokens } else { 0 },
            facts.model.as_deref().filter(|_| known),
            self.claude_catalog(),
        )
    }

    pub fn show_reload(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.reload_offered(id) {
            self.select(id, cx);
            self.reload_open(id, cx);
        }
    }

    pub(super) fn reload_open(&mut self, id: &str, cx: &mut Context<Self>) {
        self.row_menu = None;
        let Some(session) = self.sessions.iter().find(|s| s.agent.id == id) else {
            cx.notify();
            return;
        };
        let lead = self.reload_lead(id);
        let Some(card) = Card::open(&session.agent, lead) else {
            cx.notify();
            return;
        };
        self.reload_show(card, cx);
    }

    fn reload_show(&mut self, card: Card, cx: &mut Context<Self>) {
        self.reload.card = Some(card.clone());
        let entity = cx.new(|_| ReloadView::new(card));
        cx.subscribe(&entity, |this, view, choice: &ReloadChoice, cx| {
            let card = view.read(cx).card().clone();
            this.reload_chosen(&card, choice.0, cx);
        })
        .detach();
        self.reload_view = Some(entity.downgrade());
        self.show_sheet(entity, cx);
    }

    fn reload_sync(&mut self, cx: &mut Context<Self>) {
        let Some(card) = self.reload.card.clone() else {
            self.reload_view = None;
            return;
        };
        if let Some(view) = self.reload_view.as_ref().and_then(|v| v.upgrade()) {
            view.update(cx, |view, cx| view.show(card, cx));
        }
    }

    fn reload_close(&mut self, cx: &mut Context<Self>) {
        self.reload.card = None;
        self.reload_view = None;
        self.sheet = None;
        cx.notify();
    }

    fn reload_chosen(&mut self, card: &Card, act: Act, cx: &mut Context<Self>) {
        let id = card.session.clone();
        match act {
            Act::Reload | Act::TryAgain => self.reload_start(&id, cx),
            Act::Cancel | Act::Close => self.reload_close(cx),
            Act::CancelReload => {
                self.reload_send(Request::reload_cancel(&id), id, Sent::Poll, cx);
                self.reload_close(cx);
            }
            Act::CopyCommand => {
                cx.write_to_clipboard(ClipboardItem::new_string(card.command.clone()));
                self.reload_close(cx);
            }
        }
    }

    fn reload_start(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.remote_refused(id, crate::chat::remote_control::Action::Slash) {
            self.reload_close(cx);
            return;
        }
        if !self.acting(cx).control().enabled {
            self.tools.error = self.acting(cx).control().tooltip.map(str::to_owned);
            self.reload_close(cx);
            return;
        }
        if let Some(card) = self.reload.card.as_mut() {
            card.stage = Stage::Waiting(notchling_engine::reload::Wait::Turn);
        }
        self.reload.last_poll_ms = super::now_ms();
        self.reload_sync(cx);
        self.reload_send(
            Request::reload(id, true),
            id.to_owned(),
            Sent::Start,
            cx,
        );
        cx.notify();
    }

    fn reload_send(&mut self, request: Request, row: String, sent: Sent, cx: &mut Context<Self>) {
        let actions = self.acts(cx).actions;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&request) })
                .await;
            this.update(cx, |view, cx| {
                view.reload.asking.remove(&row);
                view.reload_answered(&row, result, sent, cx);
            })
            .ok();
        })
        .detach();
    }

    fn reload_answered(
        &mut self,
        id: &str,
        result: Result<Value, String>,
        sent: Sent,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(reply) => {
                if let Some(status) = reload::parse(&reply) {
                    match self.reload.apply(id, &status, super::now_ms()) {
                        Applied::Changed => self.reload_sync(cx),
                        Applied::Finished { .. } => {
                            self.reload_ack(id, cx);
                            self.reload_close(cx);
                        }
                        Applied::Nothing => {}
                    }
                }
            }
            Err(message) => match sent {
                Sent::Start => {
                    self.reload.card = None;
                    self.reload_view = None;
                    self.sheet = None;
                    self.tools.error = Some(message);
                }
                Sent::Poll => {
                    if self.reload.poll_failed() {
                        self.tools.error = Some(message);
                        self.reload_close(cx);
                    }
                }
            },
        }
        cx.notify();
    }

    fn reload_ack(&mut self, id: &str, cx: &mut Context<Self>) {
        let actions = self.acts(cx).actions;
        let request = Request::reload_state(id, true);
        cx.background_executor()
            .spawn(async move {
                let _ = actions.send(&request);
            })
            .detach();
    }

    fn reload_refresh_lead(&mut self, cx: &mut Context<Self>) {
        let Some(card) = self.reload.card.as_ref() else {
            return;
        };
        if card.stage != Stage::Ask || self.selected.as_deref() != Some(card.session.as_str()) {
            return;
        }
        let lead = self.reload_lead(&card.session);
        if lead != card.lead {
            if let Some(card) = self.reload.card.as_mut() {
                card.lead = lead;
            }
            self.reload_sync(cx);
        }
    }

    pub(super) fn reload_tick(&mut self, cx: &mut Context<Self>) {
        self.reload_refresh_lead(cx);
        let now = super::now_ms();
        if !self.reload.due(now) {
            return;
        }
        self.reload.last_poll_ms = now;
        let Some(id) = self.reload.card.as_ref().map(|c| c.session.clone()) else {
            return;
        };
        if self.reload.asking.insert(id.clone()) {
            self.reload_send(Request::reload_state(&id, false), id, Sent::Poll, cx);
        }
    }
}
