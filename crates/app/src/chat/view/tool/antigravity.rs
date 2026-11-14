use gpui::{App, Context};
use notchling_locale as t;

use super::{active_email, AccountButton, ChatTool};
use crate::accounts::model::ANTIGRAVITY;
use crate::acting::Request;
use crate::chat::data::Session;
use crate::chat::limit_ring as rules;
use crate::chat::pickers::{self, agy_option, Menu};
use crate::chat::view::limit_ring::Shown;
use crate::chat::view::ChatView;
use crate::tools::requests::{agy_cancel, agy_send, agy_send_draft, is_draft};

pub(super) struct Antigravity;

impl ChatTool for Antigravity {
    fn placeholder(&self) -> &'static str {
        t::chat::message_antigravity_placeholder()
    }

    fn refusal(&self, view: &ChatView, session: &Session) -> Option<&'static str> {
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
            agy_send(&session.agent.id, text)
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
