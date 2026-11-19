use gpui::{AppContext, Context};
use notchling_engine::sessions::AgentTarget;
use serde_json::Value;

use super::ChatView;
use crate::acting::Request;
use crate::chat::chrome::{self, Act, Applied, Sheet};
use crate::chat::data::Session;
use crate::sheets::chrome_view::{ChromeChoice, ChromeView};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sent {
    Start,
    Poll,
    Peek,
}

impl ChatView {
    pub(super) fn chrome_session(&self, id: &str) -> Option<Session> {
        self.sessions
            .iter()
            .find(|s| s.agent.id == id)
            .filter(|s| chrome::offered(s))
            .cloned()
    }

    pub(super) fn chrome_menu_item(&self, id: &str) -> Option<bool> {
        self.chrome_session(id)?;
        Some(self.use_chrome.on.get(id).copied().unwrap_or(false))
    }

    pub(super) fn chrome_note(&self) -> Option<String> {
        let id = self.selected.as_deref()?;
        self.use_chrome.note(id, super::now_ms())
    }

    pub(super) fn chrome_command(
        &mut self,
        session: &Session,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(which) = notchling_engine::chrome::command::parse(text) else {
            return false;
        };
        let id = session.agent.id.clone();
        if self.remote_refused(&id, crate::chat::remote_control::Action::Slash) {
            cx.notify();
            return true;
        }
        if !chrome::offered(session) {
            self.tools.error = Some(notchling_locale::chat::chrome_restartable().to_owned());
            cx.notify();
            return true;
        }
        if !self.acting(cx).control().enabled {
            self.tools.error = self.acting(cx).control().tooltip.map(str::to_owned);
            cx.notify();
            return true;
        }
        self.put_draft(&id, String::new(), cx);
        let on = which == notchling_engine::chrome::command::Command::On;
        self.chrome_start(&id, on, false, cx);
        true
    }

    pub(super) fn chrome_toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        self.row_menu = None;
        if self.chrome_session(id).is_none() {
            cx.notify();
            return;
        }
        let on = !self.use_chrome.on.get(id).copied().unwrap_or(false);
        self.chrome_start(id, on, false, cx);
    }

    pub(super) fn chrome_start(&mut self, id: &str, on: bool, force: bool, cx: &mut Context<Self>) {
        if self.remote_refused(id, crate::chat::remote_control::Action::Slash) {
            cx.notify();
            return;
        }
        if !self.acting(cx).control().enabled {
            self.tools.error = self.acting(cx).control().tooltip.map(str::to_owned);
            cx.notify();
            return;
        }
        self.use_chrome.begin(id, on, super::now_ms());
        self.use_chrome.last_poll_ms = super::now_ms();
        let row = id.to_owned();
        self.chrome_send(Request::chrome(id, on, force), row, Sent::Start, cx);
        cx.notify();
    }

    pub(super) fn chrome_peek(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.chrome_session(id).is_none() || self.use_chrome.asking.contains(id) {
            return;
        }
        self.use_chrome.asking.insert(id.to_owned());
        self.chrome_send(
            Request::chrome_state(id, false),
            id.to_owned(),
            Sent::Peek,
            cx,
        );
    }

    fn chrome_send(&mut self, request: Request, row: String, sent: Sent, cx: &mut Context<Self>) {
        let actions = self.acts(cx).actions;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&request) })
                .await;
            this.update(cx, |view, cx| {
                view.use_chrome.asking.remove(&row);
                view.chrome_answered(&row, result, sent, cx);
            })
            .ok();
        })
        .detach();
    }

    fn chrome_answered(
        &mut self,
        id: &str,
        result: Result<Value, String>,
        sent: Sent,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(reply) => {
                let value = reply.get("chrome").unwrap_or(&reply);
                if let Some(status) = chrome::parse(value) {
                    let applied = self.use_chrome.apply(id, &status, super::now_ms());
                    match applied {
                        Applied::Needs(sheet) => {
                            self.chrome_ack(id, cx);
                            self.chrome_open_sheet(sheet, cx);
                        }
                        Applied::Finished => self.chrome_ack(id, cx),
                        Applied::Nothing | Applied::Working => {}
                    }
                }
            }
            Err(message) => match sent {
                Sent::Start => {
                    self.use_chrome.tasks.remove(id);
                    self.tools.error = Some(message);
                }
                Sent::Poll => {
                    if self.use_chrome.poll_failed(id) {
                        self.tools.error = Some(message);
                    }
                }
                Sent::Peek => {}
            },
        }
        cx.notify();
    }

    fn chrome_ack(&mut self, id: &str, cx: &mut Context<Self>) {
        let actions = self.acts(cx).actions;
        let request = Request::chrome_state(id, true);
        cx.background_executor()
            .spawn(async move {
                let _ = actions.send(&request);
            })
            .detach();
    }

    pub(super) fn chrome_tick(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected.clone() {
            if self.chrome_session(&id).is_some() && self.use_chrome.peeked.insert(id.clone()) {
                self.chrome_peek(&id, cx);
            }
        }
        let now = super::now_ms();
        if !self.use_chrome.due(now) {
            return;
        }
        self.use_chrome.last_poll_ms = now;
        for id in self.use_chrome.active() {
            if self.use_chrome.asking.insert(id.clone()) {
                self.chrome_send(Request::chrome_state(&id, false), id, Sent::Poll, cx);
            }
        }
    }

    fn chrome_open_sheet(&mut self, sheet: Sheet, cx: &mut Context<Self>) {
        let entity = cx.new(|_| ChromeView::new(sheet));
        cx.subscribe(&entity, |this, view, choice: &ChromeChoice, cx| {
            let sheet = view.read(cx).sheet().clone();
            this.chrome_chosen(&sheet, choice.0, cx);
        })
        .detach();
        self.show_sheet(entity, cx);
    }

    fn chrome_chosen(&mut self, sheet: &Sheet, act: Act, cx: &mut Context<Self>) {
        self.sheet = None;
        let id = sheet.session.clone();
        match act {
            Act::TryAgain => self.chrome_start(&id, sheet.wanted_on, false, cx),
            Act::TurnOnAnyway => self.chrome_start(&id, true, true, cx),
            Act::TurnOffAnyway => self.chrome_start(&id, false, true, cx),
            Act::TurnOn => self.chrome_start(&id, true, false, cx),
            Act::TurnOff => self.chrome_start(&id, false, false, cx),
            Act::OpenTerminal => {
                let Some(session) = self.chrome_session(&id) else {
                    cx.notify();
                    return;
                };
                if let AgentTarget::Job(job) = &session.agent.target {
                    let opened = self.open_menu_command(
                        job,
                        session.agent.config_dir.as_deref(),
                        &session.agent.cwd,
                        "/chrome",
                        cx,
                    );
                    if let Err(message) = opened {
                        self.tools.error = Some(message);
                    }
                }
            }
        }
        cx.notify();
    }
}
