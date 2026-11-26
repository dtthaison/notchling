use std::collections::HashSet;

use notchling_engine::sessions::history::{self, Page, Query};

use super::*;
use crate::chat::sidebar_completed::{hidden_ids, without_hidden, Request};

impl ChatView {
    pub(super) fn toggle_completed(&mut self, cx: &mut Context<Self>) {
        self.completed_open = !self.completed_open;
        if self.completed_open {
            let request = self.past.open_request();
            self.fetch_past(request, cx);
        }
        self.gate.changed();
        cx.notify();
    }

    pub(super) fn preview_completed(&mut self, short: bool, cx: &mut Context<Self>) {
        self.completed_open = true;
        let roots = self.root.roots();
        let skip = hidden_ids(&self.sessions, &self.past);
        let sources = history::sources(&roots);
        let limit = if short { 4 } else { history::PAGE };
        let page = self.past.history.page(&Query {
            sources: &sources,
            ended: Vec::new(),
            skip: &skip,
            since_ms: None,
            keep: &|_, _| true,
            after: None,
            limit,
        });
        let request = self.past.begin(None, limit, true);
        self.past_loaded(&request, page, cx);
        self.past.refreshed_ms = i64::MAX;
    }

    pub(super) fn load_more_past(&mut self, cx: &mut Context<Self>) {
        if let Some(request) = self.past.more_request() {
            self.fetch_past(request, cx);
        }
    }

    pub(super) fn refresh_past(&mut self, cx: &mut Context<Self>) {
        if !self.completed_open {
            return;
        }
        if let Some(request) = self.past.watch_request(now_ms()) {
            self.fetch_past(request, cx);
        }
    }

    fn fetch_past(&mut self, request: Request, cx: &mut Context<Self>) {
        let history = self.past.history.clone();
        let roots = self.root.roots();
        let skip: HashSet<String> = hidden_ids(&self.sessions, &self.past);
        cx.spawn(async move |this, cx| {
            let ask = request.clone();
            let page = cx
                .background_executor()
                .spawn(async move {
                    let sources = history::sources(&roots);
                    history.page(&Query {
                        sources: &sources,
                        ended: Vec::new(),
                        skip: &skip,
                        since_ms: None,
                        keep: &|_, _| true,
                        after: ask.after.as_deref(),
                        limit: ask.limit,
                    })
                })
                .await;
            this.update(cx, |view, cx| view.past_loaded(&request, page, cx))
                .ok();
        })
        .detach();
    }

    fn past_loaded(&mut self, request: &Request, page: Page, cx: &mut Context<Self>) {
        if !self.past.accepts(request) {
            return;
        }
        let hidden = hidden_ids(&self.sessions, &self.past);
        if request.replace {
            let past = &self.past;
            self.sessions.retain(|s| !past.holds(s));
            let keep = self.selected.clone();
            self.past.replace(
                page.items.clone(),
                page.next.clone(),
                now_ms(),
                keep.as_deref(),
            );
        } else {
            self.past.add(page.items.clone(), page.next.clone());
        }
        for session in without_hidden(self.past.sessions(), &hidden) {
            if !self.sessions.iter().any(|s| s.agent.id == session.agent.id) {
                self.sessions.push(session);
            }
        }
        if self.completed_open {
            let hidden = hidden_ids(&self.sessions, &self.past);
            if let Some(more) = self.past.refill(&page.items, &hidden, page.next.as_deref()) {
                self.fetch_past(more, cx);
            }
        }
        self.gate.changed();
        cx.notify();
    }
}
