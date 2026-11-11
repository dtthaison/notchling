//! Claude Code behind Chat's parts: replies and Stop through the daemon's `session.reply` and
//! `session.interrupt`, the pickers as `/model` and `/effort` replies, the account menu of lanes,
//! and the main login's 5-hour and weekly limits.

use gpui::Context;

use super::{AccountButton, ChatTool};
use crate::accounts::model::CLAUDE_CODE;
use crate::acting::Request;
use crate::chat::claude_picker::Kind;
use crate::chat::data::Session;
use crate::chat::limit_ring as rules;
use crate::chat::pickers::{self, Menu, EFFORT};
use crate::chat::terminal::{menu_route, MenuRoute};
use crate::chat::view::limit_ring::Shown;
use crate::chat::view::tools::{EXITED, READ_ONLY};
use crate::chat::view::ChatView;
use notchling_engine::sessions::AgentGroup;

pub(super) struct Claude;

impl ChatTool for Claude {
    fn placeholder(&self) -> &'static str {
        "Reply…"
    }

    fn refusal(&self, _view: &ChatView, session: &Session) -> Option<&'static str> {
        if session.agent.label == "Exited" {
            Some(EXITED)
        } else if !session.agent.can_reply() {
            Some(READ_ONLY)
        } else {
            None
        }
    }

    fn working(&self, session: &Session) -> bool {
        session.agent.group == AgentGroup::Working
    }

    /// A command that opens a menu runs in a terminal tab attached to the session, where the menu
    /// shows; typed through the hidden attach it would hold the session on a menu unseen.
    fn intercept(
        &self,
        view: &mut ChatView,
        session: &Session,
        text: &str,
        cx: &mut Context<ChatView>,
    ) -> bool {
        let id = &session.agent.id;
        match menu_route(text, &session.agent.target) {
            MenuRoute::Reply => return false,
            MenuRoute::Attach { job } => {
                let opened = view.open_menu_command(
                    &job,
                    session.agent.config_dir.as_deref(),
                    &session.agent.cwd,
                    text,
                    cx,
                );
                match opened {
                    Ok(()) => {
                        view.put_draft(id, String::new(), cx);
                        view.set_attached(id, &[], cx);
                    }
                    Err(message) => view.tools.error = Some(message),
                }
            }
            MenuRoute::OwnTerminal => {
                view.tools.error = Some(format!(
                    "{} opens a menu: run it in this session's own terminal",
                    text.split_whitespace().next().unwrap_or("This command")
                ));
            }
        }
        cx.notify();
        true
    }

    fn send(&self, _view: &ChatView, session: &Session, text: &str) -> Request {
        Request::reply(&session.agent.id, text)
    }

    fn stop(&self, id: &str) -> Request {
        Request::interrupt(id)
    }

    fn menus(&self, view: &ChatView, _session: &Session) -> Vec<Menu> {
        pickers::claude(
            view.claude_model(),
            view.claude_effort(),
            view.claude_catalog(),
        )
    }

    /// The model list is read again as a menu opens: the daemon fetches Anthropic's list only when
    /// asked, so a new model shows without waiting for the hourly read.
    fn menu_opening(&self, view: &mut ChatView) {
        view.source.ask_catalog();
    }

    fn pick(&self, view: &mut ChatView, menu: &str, value: &str, cx: &mut Context<ChatView>) {
        let kind = if menu == EFFORT {
            Kind::Effort
        } else {
            Kind::Model
        };
        view.pick_claude(kind, value, cx);
    }

    fn account(&self, view: &ChatView) -> Option<AccountButton> {
        let menu = view.menu()?;
        let text = match &menu.lane {
            Some(_) => format!("{} · lane", menu.label),
            None => menu.label.clone(),
        };
        Some(AccountButton {
            text,
            tip: menu.tooltip,
        })
    }

    fn account_click(&self, view: &mut ChatView, cx: &mut Context<ChatView>) {
        view.toggle_account_menu(cx);
    }

    /// The main login's windows; a session in a lane has none yet.
    fn limits(&self, view: &ChatView, session: &Session) -> Option<Shown> {
        if session.agent.config_dir.is_some() {
            return None;
        }
        let s = view.accounts.snapshot();
        let windows = &s
            .limits
            .providers
            .iter()
            .find(|p| p.id == "claude")?
            .windows;
        let limits = rules::claude_limits(windows);
        Some(Shown {
            arc: rules::arc(&limits),
            limits,
            plan: s
                .claude
                .active
                .as_ref()
                .and_then(|id| s.claude.accounts.iter().find(|a| &a.id == id))
                .and_then(|a| a.plan.clone()),
            provider: CLAUDE_CODE,
        })
    }
}
