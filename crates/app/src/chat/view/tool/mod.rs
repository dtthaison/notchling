use gpui::{App, Context};

use super::limit_ring::Shown;
use super::ChatView;
use crate::acting::Request;
use crate::chat::data::{Session, Tool};
use crate::chat::pickers::Menu;

mod antigravity;
mod claude;
mod codex;

pub(super) struct AccountButton {
    pub text: String,
    pub tip: String,
}

pub(super) trait ChatTool: Sync {
    fn placeholder(&self) -> &'static str;

    fn refusal(&self, view: &ChatView, session: &Session) -> Option<&'static str>;

    fn working(&self, _session: &Session) -> bool {
        false
    }

    fn intercept(
        &self,
        _view: &mut ChatView,
        _session: &Session,
        _text: &str,
        _cx: &mut Context<ChatView>,
    ) -> bool {
        false
    }

    fn send(&self, view: &ChatView, session: &Session, text: &str) -> Request;

    fn queues(&self) -> bool {
        false
    }

    fn stop(&self, id: &str) -> Request;

    fn menus(&self, view: &ChatView, session: &Session, cx: &App) -> Vec<Menu>;

    fn menu_opening(&self, _view: &mut ChatView) {}

    fn pick(&self, view: &mut ChatView, menu: &str, value: &str, cx: &mut Context<ChatView>);

    fn account(&self, view: &ChatView) -> Option<AccountButton>;

    fn account_click(&self, view: &mut ChatView, cx: &mut Context<ChatView>);

    fn limits(&self, view: &ChatView, session: &Session) -> Option<Shown>;
}

pub(super) fn of(tool: Tool) -> &'static dyn ChatTool {
    match tool {
        Tool::Claude => &claude::Claude,
        Tool::Codex => &codex::Codex,
        Tool::Antigravity => &antigravity::Antigravity,
    }
}

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
