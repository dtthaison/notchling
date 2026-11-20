use std::path::PathBuf;

use gpui::Context;
use notchling_engine::sessions::AgentGroup;

use super::ChatView;
use crate::chat::busy::{self, InFlight, Reading};
use crate::chat::outbox::Bubble;
use crate::phone::bridge::now_ms;

impl ChatView {
    pub(super) fn explain_busy(&self, bubbles: &mut [Bubble]) {
        if self.outbox_preview.is_some() {
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        let flight = self.busy.of(&session.agent);
        busy::explain(bubbles, &busy::facts(&session.agent, flight));
    }

    pub(super) fn read_busy(&mut self, cx: &mut Context<Self>) {
        if self.outbox_preview.is_some() || self.root.fixture.is_some() {
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        let agent = session.agent.clone();
        let Some(key) = Reading::key_of(&agent) else {
            return;
        };
        if agent.group != AgentGroup::Working || !self.busy.due(&key, now_ms()) {
            return;
        }
        let waiting = self.source.outbox_bubbles(&agent, &self.conversation);
        if !busy::wants_read(&agent, &waiting) {
            return;
        }
        let Some(job) = busy::job_of(&agent) else {
            return;
        };
        let dir = agent
            .config_dir
            .as_deref()
            .map_or_else(|| self.root.claude_dir.clone(), PathBuf::from);
        let file = dir.join("jobs").join(job).join("state.json");
        self.busy.begin(&key, now_ms());
        cx.spawn(async move |this, cx| {
            let flight = cx
                .background_executor()
                .spawn(async move { read_in_flight(&file) })
                .await;
            this.update(cx, |view, cx| {
                view.busy.finish(&key, flight);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

fn read_in_flight(file: &std::path::Path) -> InFlight {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .map(|job| busy::in_flight(&job))
        .unwrap_or_default()
}
