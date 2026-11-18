use gpui::{App, ClipboardItem, Context};
use notchling_locale as t;

use super::super::outbox_rows::send_request;
use super::{active_email, AccountButton, ChatTool};
use crate::accounts::model::ANTIGRAVITY;
use crate::acting::Request;
use crate::chat::data::Session;
use crate::chat::limit_ring as rules;
use crate::chat::pickers::{self, agy_option, Menu};
use crate::chat::view::limit_ring::Shown;
use crate::chat::view::ChatView;
use crate::tools::agy_hand_off::{self, Target, Words};
use crate::tools::requests::{agy_cancel, agy_send_draft, is_draft};
use notchling_engine::contract::channel::Opener;
use notchling_engine::contract::channel_draft;

pub(super) struct Antigravity;

struct Mac<'a> {
    cx: &'a mut App,
    panel: Option<String>,
}

impl Opener for Mac<'_> {
    fn installed(&self) -> bool {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        channel_draft::installed_in(&channel_draft::app_folders(home.as_deref()))
    }

    fn open(&mut self, url: &str) {
        self.cx.open_url(url);
    }

    fn panel(&mut self, url: &str) {
        self.panel = Some(url.to_owned());
    }

    fn copy(&mut self, text: &str) {
        self.cx
            .write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
    }
}

impl ChatTool for Antigravity {
    fn placeholder(&self) -> &'static str {
        t::chat::message_antigravity_placeholder()
    }

    fn hand_off(&self, view: &ChatView) -> Option<Words> {
        agy_hand_off::words(view.tools.agy_channel())
    }

    fn intercept(
        &self,
        view: &mut ChatView,
        session: &Session,
        text: &str,
        cx: &mut Context<ChatView>,
    ) -> bool {
        let email = active_email(view, ANTIGRAVITY);
        let remote_root = agy_hand_off::remote_root(&view.root.antigravity_dir);
        let target = Target {
            in_panel: agy_hand_off::in_panel(&view.tools.agy_remote_in),
            remote_root: &remote_root,
            email: email.as_deref(),
            row_id: &session.agent.id,
        };
        let channel = view.tools.agy_channel();
        let mut mac = Mac {
            cx: &mut *cx,
            panel: None,
        };
        let Some(done) = agy_hand_off::hand_over(channel, &mut mac, &target, text) else {
            return false;
        };
        let panel = mac.panel.take();
        if let Some(url) = panel {
            view.open_remote_panel(url, cx);
        }
        view.tools.error = done.err().map(str::to_owned);
        view.tools.agy_note = done.ok().map(|note| (session.agent.id.clone(), note));
        cx.notify();
        true
    }

    fn refusal(&self, view: &ChatView, session: &Session) -> Option<&'static str> {
        if self.hand_off(view).is_some() {
            return None;
        }
        if let Some(note) = view.tools.agy.as_ref().and_then(|a| a.elsewhere.as_deref()) {
            return Some(elsewhere_note(note));
        }
        view.tools
            .live
            .is_working(&session.agent.id)
            .then_some(t::chat::agy_busy())
    }

    fn send(&self, _view: &ChatView, session: &Session, text: &str) -> Request {
        if is_draft(&session.agent.id) {
            agy_send_draft(&session.agent.cwd, text)
        } else {
            send_request(&session.agent.id, text, None)
        }
    }

    fn stop(&self, id: &str) -> Request {
        agy_cancel(id)
    }

    fn menus(&self, view: &ChatView, session: &Session, _cx: &App) -> Vec<Menu> {
        pickers::antigravity(
            &view.tools.agy_config,
            !is_draft(&session.agent.id),
            view.tools.agy.as_ref().is_some_and(|a| a.loading),
        )
    }

    fn pick(&self, view: &mut ChatView, menu: &str, value: &str, cx: &mut Context<ChatView>) {
        view.set_agy_option(agy_option(menu), value, cx);
    }

    fn account(&self, view: &ChatView) -> Option<AccountButton> {
        Some(AccountButton {
            text: active_email(view, ANTIGRAVITY)?,
            tip: t::chat::antigravity_account_tip().to_owned(),
        })
    }

    fn account_click(&self, view: &mut ChatView, cx: &mut Context<ChatView>) {
        view.open_accounts_sheet(Some(ANTIGRAVITY), cx);
    }

    fn limits(&self, view: &ChatView, _session: &Session) -> Option<Shown> {
        let s = view.accounts.snapshot();
        let agy = s.providers.providers.iter().find(|p| p.id == ANTIGRAVITY)?;
        let active = agy.active_id.as_ref()?;
        let report = s.usage.get(ANTIGRAVITY)?.get(active)?;
        let model = view
            .tools
            .agy_config
            .iter()
            .find(|o| o.id == "model")
            .map(|o| o.current_name().to_owned());
        let limits = rules::antigravity_limits(report, model.as_deref());
        Some(Shown {
            arc: limits.first().map(rules::arc_of),
            limits,
            plan: agy
                .accounts
                .iter()
                .find(|a| &a.id == active)
                .and_then(|a| a.plan.clone()),
            provider: ANTIGRAVITY,
        })
    }
}

fn elsewhere_note(note: &str) -> &'static str {
    use notchling_engine::tools::agy::elsewhere::{DELEGATION_NOTE, OTHER_NOTE};
    if note == DELEGATION_NOTE {
        DELEGATION_NOTE
    } else {
        OTHER_NOTE
    }
}
