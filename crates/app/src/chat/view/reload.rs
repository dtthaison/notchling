use gpui::{AppContext, ClipboardItem, Context};
use serde_json::Value;

use super::ChatView;
use crate::acting::Request;
use crate::chat::session_reload::{self, Act, Applied, Card, Stage, Via};
use crate::sheets::reload_view::{ReloadChoice, ReloadView};
use crate::sheets::view::Dismiss;

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
        let Some(session) = self.sessions.iter().find(|s| s.agent.id == id) else {
            return;
        };
        let lead = self.reload_lead(id);
        let Some(mut card) = Card::open(&session.agent, lead) else {
            cx.notify();
            return;
        };
        let live = self.reload.live(id);
        if live && card.stage == Stage::Ask {
            card.stage = Stage::Waiting(notchling_engine::reload::Wait::Turn);
        }
        let ask = card.stage != Stage::Terminal;
        self.reload_show(card, cx);
        if ask && self.reload.asking.insert(id.to_owned()) {
            let gen = self.reload.generation(id);
            self.reload_send(
                Request::reload_state(id, false),
                id.to_owned(),
                Via::Open,
                gen,
                cx,
            );
        }
    }

    fn reload_show(&mut self, card: Card, cx: &mut Context<Self>) {
        self.reload.card = Some(card.clone());
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
                let gen = self.reload.cancelling(&id);
                self.reload_send(Request::reload_cancel(&id), id, Via::Cancel, gen, cx);
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
        let gen = self.reload.start(id, super::now_ms());
        self.reload_sync(cx);
        self.reload_send(
            Request::reload(id, true),
            id.to_owned(),
            Via::Start,
            gen,
            cx,
        );
        cx.notify();
    }

    fn reload_send(
        &mut self,
        request: Request,
        row: String,
        via: Via,
        gen: u64,
        cx: &mut Context<Self>,
    ) {
        let actions = self.acts(cx).actions;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&request) })
                .await;
            this.update(cx, |view, cx| {
                if view.reload.generation(&row) == gen {
                    view.reload.asking.remove(&row);
                }
                view.reload_answered(&row, result, via, gen, cx);
            })
            .ok();
        })
        .detach();
    }

    fn reload_shown(&self, id: &str) -> bool {
        self.reload_view.is_some() && self.reload.card.as_ref().is_some_and(|c| c.session == id)
    }

    fn reload_answered(
        &mut self,
        id: &str,
        result: Result<Value, String>,
        via: Via,
        gen: u64,
        cx: &mut Context<Self>,
    ) {
        let shown = self.reload_shown(id);
        match result {
            Ok(reply) => {
                if let Some(status) = session_reload::parse(&reply) {
                    match self
                        .reload
                        .apply_via(id, &status, super::now_ms(), via, gen)
                    {
                        Applied::Changed => self.reload_sync(cx),
                        Applied::Stale => self.reload_ack(id, cx),
                        Applied::Finished { followed } => {
                            self.reload_ack(id, cx);
                            if shown {
                                self.reload_close(cx);
                            }
                            if let Some(next) = followed {
                                if self.selected.as_deref() == Some(id) {
                                    self.select(&next, cx);
                                }
                            }
                        }
                        Applied::Nothing => {}
                    }
                }
            }
            Err(message) if self.reload.generation(id) == gen => match via {
                Via::Start => {
                    self.reload.stop(id);
                    if shown {
                        self.reload_close(cx);
                    }
                    self.tools.error = Some(message);
                }
                Via::Cancel => self.tools.error = Some(message),
                Via::Poll | Via::Open => {
                    if self.reload.poll_failed(id) {
                        self.tools.error = Some(message);
                        if shown {
                            self.reload_close(cx);
                        }
                    }
                }
            },
            Err(_) => {}
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
        self.reload_scan(now, cx);
        if !self.reload.due(now) {
            return;
        }
        self.reload.last_poll_ms = now;
        for id in self.reload.polls() {
            if self.reload.asking.insert(id.clone()) {
                let gen = self.reload.generation(&id);
                self.reload_send(Request::reload_state(&id, false), id, Via::Poll, gen, cx);
            }
        }
    }

    fn reload_scan(&mut self, now: i64, cx: &mut Context<Self>) {
        if !self.reload.scan_due(now) {
            return;
        }
        self.reload.scanning = true;
        self.reload.last_scan_ms = now;
        let actions = self.acts(cx).actions;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&Request::reload_active()) })
                .await;
            this.update(cx, |view, cx| {
                view.reload.scanning = false;
                let ids: Vec<String> = result
                    .ok()
                    .and_then(|reply| reply.get("active").cloned())
                    .and_then(|active| serde_json::from_value(active).ok())
                    .unwrap_or_default();
                if !ids.is_empty() {
                    view.reload.seen_active(&ids);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}
