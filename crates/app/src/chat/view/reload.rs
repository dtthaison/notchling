use gpui::{AppContext, ClipboardItem, Context};
use notchling_engine::reload::Need;
use serde_json::Value;

use super::ChatView;
use crate::acting::Request;
use crate::chat::session_reload::{self, Act, Card, Intent, Settle, Synced};
use crate::sheets::reload_view::{ReloadChoice, ReloadView};
use crate::sheets::view::Dismiss;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sent {
    Start,
    Cancel,
}

impl ChatView {
    pub(super) fn reload_mode(&self, id: &str) -> Option<session_reload::Mode> {
        self.sessions
            .iter()
            .find(|s| s.agent.id == id)
            .and_then(session_reload::mode)
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
        session_reload::lead(
            if known { facts.context_tokens } else { 0 },
            facts.model.as_deref().filter(|_| known),
            self.claude_catalog(),
        )
    }

    pub fn show_reload(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.reload_offered(id) {
            self.reload_open(id, cx);
        }
    }

    pub(super) fn reload_open(&mut self, id: &str, cx: &mut Context<Self>) {
        self.row_menu = None;
        if !self.sessions.iter().any(|s| s.agent.id == id) {
            cx.notify();
            return;
        }
        self.select(id, cx);
        self.reload_follow_feed(cx);
        let Some(session) = self.sessions.iter().find(|s| s.agent.id == id) else {
            return;
        };
        let lead = self.reload_lead(id);
        let floor = self.reload.feed.rev(id);
        let Some(card) = Card::open(&session.agent, lead, floor) else {
            cx.notify();
            return;
        };
        let card = self.reload.open(card);
        self.reload_show(card, cx);
    }

    fn reload_show(&mut self, card: Card, cx: &mut Context<Self>) {
        let entity = cx.new(|_| ReloadView::new(card));
        cx.subscribe(&entity, |this, view, choice: &ReloadChoice, cx| {
            let card = view.read(cx).card().clone();
            this.reload_chosen(&card, choice.0, cx);
        })
        .detach();
        cx.subscribe(&entity, |this, _, _: &Dismiss, _| {
            this.reload.card = None;
            this.reload_view = None;
        })
        .detach();
        self.reload_view = Some(entity.downgrade());
        self.show_fitted(entity, crate::sheets::reload_view::WIDTH, cx);
    }

    fn reload_shown(&self) -> bool {
        let Some(view) = self.reload_view.as_ref().and_then(|v| v.upgrade()) else {
            return false;
        };
        self.sheet
            .as_ref()
            .is_some_and(|sheet| sheet.view.entity_id() == view.entity_id())
    }

    fn reload_sync_view(&mut self, cx: &mut Context<Self>) {
        let Some(card) = self.reload.card.clone() else {
            return;
        };
        if let Some(view) = self.reload_view.as_ref().and_then(|v| v.upgrade()) {
            view.update(cx, |view, cx| view.show(card, cx));
        }
    }

    fn reload_close(&mut self, cx: &mut Context<Self>) {
        let shown = self.reload_shown();
        self.reload.card = None;
        self.reload_view = None;
        if shown {
            self.sheet = None;
        }
        cx.notify();
    }

    fn reload_chosen(&mut self, card: &Card, act: Act, cx: &mut Context<Self>) {
        let id = card.session.clone();
        match act {
            Act::Reload | Act::TryAgain => self.reload_start(&id, cx),
            Act::Cancel | Act::Close => self.reload_close(cx),
            Act::CancelReload => self.reload_cancel(&id, cx),
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
        let floor = self.reload.feed.rev(id);
        let Some(card) = self.reload.card_for(id) else {
            return;
        };
        card.begin(floor);
        self.reload_settle_card(cx);
        self.reload_send(Request::reload(id, true), id.to_owned(), Sent::Start, cx);
    }

    fn reload_cancel(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(card) = self.reload.card_for(id) else {
            return;
        };
        card.intent = Intent::Cancelling;
        self.reload_settle_card(cx);
        self.reload_send(Request::reload_cancel(id), id.to_owned(), Sent::Cancel, cx);
    }

    fn reload_send(&mut self, request: Request, id: String, sent: Sent, cx: &mut Context<Self>) {
        let actions = self.acts(cx).actions;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&request) })
                .await;
            this.update(cx, |view, cx| view.reload_answered(&id, result, sent, cx))
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
        if let Ok(reply) = &result {
            self.source.absorb_reload(id, reply);
        }
        let waiting = match sent {
            Sent::Start => Intent::Starting,
            Sent::Cancel => Intent::Cancelling,
        };
        let gone = matches!(&result, Ok(reply) if reply.get("task").is_none_or(Value::is_null));
        let Some(card) = self.reload.card_for(id).filter(|c| c.intent == waiting) else {
            self.reload_follow_feed(cx);
            return;
        };
        card.intent = match (sent, result) {
            (Sent::Start, Err(text)) => Intent::Ended(Need::Failed { text }),
            (Sent::Cancel, Err(text)) => Intent::Refused(text),
            (_, Ok(_)) => Intent::Following,
        };
        if sent == Sent::Cancel && gone {
            self.reload_close(cx);
            return;
        }
        self.reload_follow_feed(cx);
        self.reload_settle_card(cx);
        cx.notify();
    }

    fn reload_settle_card(&mut self, cx: &mut Context<Self>) {
        let feed = self.reload.feed.clone();
        let Some(card) = self.reload.card.as_mut() else {
            return;
        };
        match card.settle(feed.rev(&card.session), feed.task(&card.session)) {
            Settle::Close => self.reload_close(cx),
            Settle::Changed => self.reload_sync_view(cx),
            Settle::Same => {}
        }
    }

    fn reload_follow_feed(&mut self, cx: &mut Context<Self>) {
        let feed = self.source.reloads();
        if feed == self.reload.feed {
            return;
        }
        let synced = self.reload.sync(&feed, super::now_ms());
        self.reload_apply(synced, cx);
    }

    fn reload_apply(&mut self, synced: Synced, cx: &mut Context<Self>) {
        for (id, rev) in synced.acks {
            let actions = self.acts(cx).actions;
            let request = Request::reload_ack(&id, rev);
            cx.background_executor()
                .spawn(async move {
                    let _ = actions.send(&request);
                })
                .detach();
        }
        for (old, new) in synced.follows {
            if self.selected.as_deref() == Some(old.as_str()) {
                self.select(&new, cx);
            }
        }
        match synced.settle {
            Some(Settle::Close) => self.reload_close(cx),
            Some(Settle::Changed) => self.reload_sync_view(cx),
            Some(Settle::Same) | None => {}
        }
        cx.notify();
    }

    fn reload_refresh_lead(&mut self, cx: &mut Context<Self>) {
        let Some(card) = self.reload.card.as_ref() else {
            return;
        };
        if card.stage != session_reload::Stage::Ask
            || self.selected.as_deref() != Some(card.session.as_str())
        {
            return;
        }
        let lead = self.reload_lead(&card.session);
        if lead != card.lead {
            if let Some(card) = self.reload.card.as_mut() {
                card.lead = lead;
            }
            self.reload_sync_view(cx);
        }
    }

    pub(super) fn reload_tick(&mut self, cx: &mut Context<Self>) {
        if self.reload.card.is_some() && !self.reload_shown() {
            self.reload.card = None;
            self.reload_view = None;
        }
        self.reload_refresh_lead(cx);
        self.reload_follow_feed(cx);
    }
}
