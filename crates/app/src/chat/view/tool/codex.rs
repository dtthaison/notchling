//! Codex behind Chat's parts: each message is a `codex exec` turn the daemon runs (`codex.send`,
//! queued in its outbox while one runs), Stop is `codex.stop`, the pickers set the next turn's
//! model and effort, and the account and limits are the active Codex login's.

use gpui::Context;

use super::{active_email, AccountButton, ChatTool};
use crate::accounts::model::CODEX;
use crate::acting::Request;
use crate::chat::data::Session;
use crate::chat::limit_ring as rules;
use crate::chat::pickers::{self, Menu, MODEL, RESET};
use crate::chat::view::limit_ring::Shown;
use crate::chat::view::ChatView;
use crate::tools::requests::{codex_send, codex_send_draft, codex_stop, is_draft};
use notchling_engine::tools::codex::models;

pub(super) struct Codex;

impl ChatTool for Codex {
    fn placeholder(&self) -> &'static str {
        "Message Codex…"
    }

    fn refusal(&self, _view: &ChatView, _session: &Session) -> Option<&'static str> {
        None
    }

    fn send(&self, view: &ChatView, session: &Session, text: &str) -> Request {
        let id = &session.agent.id;
        let pick = view.tools.picks.get(id).cloned().unwrap_or_default();
        if is_draft(id) {
            codex_send_draft(id, &session.agent.cwd, text, &pick)
        } else {
            codex_send(id, text, &pick)
        }
    }

    fn queues(&self) -> bool {
        true
    }

    fn stop(&self, id: &str) -> Request {
        codex_stop(id)
    }

    fn menus(&self, view: &ChatView, _session: &Session) -> Vec<Menu> {
        pickers::codex(&view.codex_picker())
    }

    /// Codex's models come from its cache, read again as a menu opens.
    fn menu_opening(&self, view: &mut ChatView) {
        view.tools.codex_models = Some(models::load(&view.root.codex_dir));
    }

    fn pick(&self, view: &mut ChatView, menu: &str, value: &str, cx: &mut Context<ChatView>) {
        let mut picker = view.codex_picker();
        match menu {
            MODEL if value == RESET => picker.reset(),
            MODEL => picker.choose_model(value),
            _ => {
                let model = picker
                    .pick
                    .model
                    .clone()
                    .or_else(|| picker.running_model.clone())
                    .unwrap_or_default();
                picker.choose_effort(&model, value);
            }
        }
        view.keep_pick(&picker);
        view.tools.picker = None;
        cx.notify();
    }

    fn account(&self, view: &ChatView) -> Option<AccountButton> {
        Some(AccountButton {
            text: active_email(view, CODEX)?,
            tip: "The Codex account. Click to switch or add one.".to_owned(),
        })
    }

    fn account_click(&self, view: &mut ChatView, cx: &mut Context<ChatView>) {
        view.open_accounts_sheet(Some(CODEX), cx);
    }

    /// The active login's first group.
    fn limits(&self, view: &ChatView, _session: &Session) -> Option<Shown> {
        let s = view.accounts.snapshot();
        let codex = s.providers.providers.iter().find(|p| p.id == CODEX)?;
        let active = codex.active_id.as_ref()?;
        let report = s.usage.get(CODEX)?.get(active)?;
        let limits = rules::codex_limits(report);
        Some(Shown {
            arc: rules::arc(&limits),
            limits,
            plan: codex
                .accounts
                .iter()
                .find(|a| &a.id == active)
                .and_then(|a| a.plan.clone()),
            provider: CODEX,
        })
    }
}
