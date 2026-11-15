use gpui::Context;

use super::ChatView;
use crate::acting::Request;
use crate::chat::remote::{bring_back_job, bring_back_outcome, BringBack};
use crate::chat::remote_control::Action;

impl ChatView {
    pub(super) fn bring_back_for(&self, row: &str) -> Option<String> {
        let session = self.sessions.iter().find(|s| s.agent.id == row)?;
        bring_back_job(&session.agent)
    }

    pub(super) fn bring_back_note(&self) -> Option<&BringBack> {
        let (row, state) = self.tools.bring_back.as_ref()?;
        (self.selected.as_deref() == Some(row.as_str())).then_some(state)
    }

    pub(super) fn bring_back(&mut self, row: String, cx: &mut Context<Self>) {
        self.row_menu = None;
        if self.remote_refused(&row, Action::BringBack) {
            if let Some(note) = crate::chat::sidebar_node::reply_note(&row, &self.nodes()) {
                self.tools.error = Some(note);
            }
            cx.notify();
            return;
        }
        let Some(job) = self.bring_back_for(&row) else {
            return;
        };
        if self.tools.sending || matches!(&self.tools.bring_back, Some((_, BringBack::Busy))) {
            return;
        }
        if !self.acting(cx).control().enabled {
            return;
        }
        let request = Request::cluster_return(&job);
        let keyed = row.clone();
        self.spawn_send(
            request,
            move |view, result, _| {
                view.tools.bring_back = Some((keyed, bring_back_outcome(result)));
            },
            cx,
        );
        self.tools.bring_back = Some((row, BringBack::Busy));
    }
}
