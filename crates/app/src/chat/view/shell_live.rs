use gpui::Context;
use notchling_engine::transcript::model::Conversation;

use super::{now_ms, ChatView};
use crate::acting::Request;
use crate::chat::rows::Row;
use crate::chat::shell_live::{lines_of, pending, readable, Pending};

impl ChatView {
    fn shell_live_target(&self, cx: &mut Context<Self>) -> Option<(String, Pending)> {
        if self.root.fixture.is_some() || self.loading || self.open_subagent.is_some() {
            return None;
        }
        let session = self.selected_session()?;
        if !readable(&session.agent, session.tool) || !self.selected_working() {
            return None;
        }
        let row = session.agent.id.clone();
        if !self.acting(cx).control().enabled {
            return None;
        }
        Some((row, pending(&self.conversation)?))
    }

    pub(super) fn shell_live_row(&self, shown: &Conversation) -> Option<Row> {
        self.shell_live.row(pending(shown).as_ref())
    }
}

pub fn tick(view: &mut ChatView, cx: &mut Context<ChatView>) {
    let target = view.shell_live_target(cx);
    if view.shell_live.sync(target.as_ref().map(|(_, want)| want)) {
        view.refresh_conversation(cx);
    }
    let Some((row, want)) = target else {
        return;
    };
    if !view.shell_live.due(now_ms()) {
        return;
    }
    view.shell_live.begin();
    let actions = view.acts(cx).actions;
    cx.spawn(async move |this, cx| {
        let command = want.command.clone();
        let result = cx
            .background_executor()
            .spawn(async move { actions.send(&Request::shell_output(&row, &command)) })
            .await;
        this.update(cx, |view, cx| {
            let now = now_ms();
            let shown = match result {
                Ok(reply) => view.shell_live.arrived(&want.key, lines_of(&reply), now),
                Err(_) => {
                    view.shell_live.failed(&want.key, now);
                    false
                }
            };
            if shown {
                view.refresh_conversation(cx);
            }
        })
        .ok();
    })
    .detach();
}
