//! Chat's one interface to a session's tool (decision 0015). The reply box, its pickers, the
//! account button and the Limit Ring are drawn once, for every tool, from what a [`ChatTool`]
//! answers; each tool is one file here that maps its own API onto those answers. Nothing else in
//! Chat's views asks which tool a row is: [`of`] is the only place that turns a [`Tool`] into
//! behavior, so a new tool is a new file and a new arm there, and the compiler names every part
//! it still has to answer.
//!
//! Where a tool can't do something it still answers: a refusal with the reason, a disabled menu
//! with a tooltip. A part is never just missing for one tool (`docs/chat-composer-parity.md`).

use gpui::Context;

use super::limit_ring::Shown;
use super::ChatView;
use crate::acting::Request;
use crate::chat::data::{Session, Tool};
use crate::chat::pickers::Menu;

mod antigravity;
mod claude;
mod codex;

/// The account button's words: the login the session uses, and what a click does.
pub(super) struct AccountButton {
    pub text: String,
    pub tip: String,
}

/// What Chat's shared parts ask of a session's tool.
pub(super) trait ChatTool: Sync {
    /// The reply box's empty text for a session that has started.
    fn placeholder(&self) -> &'static str;

    /// Why the reply box takes nothing from this session now, beyond acting being off.
    fn refusal(&self, view: &ChatView, session: &Session) -> Option<&'static str>;

    /// Whether a turn runs that the `tools` topic doesn't report (Claude Code's own state).
    fn working(&self, _session: &Session) -> bool {
        false
    }

    /// Text the tool handles in Chat's own way rather than as a message (Claude Code's menu
    /// commands, which open in a terminal tab). True when it was handled.
    fn intercept(
        &self,
        _view: &mut ChatView,
        _session: &Session,
        _text: &str,
        _cx: &mut Context<ChatView>,
    ) -> bool {
        false
    }

    /// The request that sends `text` to the session.
    fn send(&self, view: &ChatView, session: &Session, text: &str) -> Request;

    /// Whether the daemon may answer a send with "queued" or "starting" (the outbox).
    fn queues(&self) -> bool {
        false
    }

    /// The request that ends the session's turn.
    fn stop(&self, id: &str) -> Request;

    /// The pickers under the box (`chat::pickers`): Model, Effort and any other choice.
    fn menus(&self, view: &ChatView, session: &Session) -> Vec<Menu>;

    /// Called as one of its menus opens, to read what the menu lists.
    fn menu_opening(&self, _view: &mut ChatView) {}

    /// A pick in one of its menus.
    fn pick(&self, view: &mut ChatView, menu: &str, value: &str, cx: &mut Context<ChatView>);

    /// The account button, or `None` while the login isn't known.
    fn account(&self, view: &ChatView) -> Option<AccountButton>;

    /// A click on the account button.
    fn account_click(&self, view: &mut ChatView, cx: &mut Context<ChatView>);

    /// The Limit Ring's limits, or `None` when the account's aren't known.
    fn limits(&self, view: &ChatView, session: &Session) -> Option<Shown>;
}

/// The tool behind a session: the one place a [`Tool`] becomes behavior.
pub(super) fn of(tool: Tool) -> &'static dyn ChatTool {
    match tool {
        Tool::Claude => &claude::Claude,
        Tool::Codex => &codex::Codex,
        Tool::Antigravity => &antigravity::Antigravity,
    }
}

/// The active login's email for an account provider (`accounts::model`'s ids).
fn active_email(view: &ChatView, provider: &str) -> Option<String> {
    let s = view.accounts.snapshot();
    let p = s.providers.providers.iter().find(|p| p.id == provider)?;
    let active = p.active_id.as_ref()?;
    p.accounts
        .iter()
        .find(|acc| &acc.id == active)
        .map(|acc| acc.email.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Tool; 3] = [Tool::Claude, Tool::Codex, Tool::Antigravity];

    #[test]
    fn every_tool_has_its_own_box_text_and_stop() {
        let texts: std::collections::HashSet<_> =
            ALL.iter().map(|t| of(*t).placeholder()).collect();
        assert_eq!(texts.len(), ALL.len());
        for t in ALL {
            assert!(!of(t).stop("row").op.is_empty(), "{t:?} has no Stop");
        }
    }

    #[test]
    fn only_codex_queues_through_the_daemon_so_far() {
        let queueing: Vec<_> = ALL.into_iter().filter(|t| of(*t).queues()).collect();
        assert_eq!(queueing, [Tool::Codex]);
    }
}
