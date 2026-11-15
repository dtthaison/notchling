use std::sync::Arc;

use gpui::{AppContext, Context};
use notchling_engine::cluster::Qualified;

use super::ChatView;
use crate::chat::sidebar_node::{reply_note, Nodes};
use crate::cluster::feed::ClusterFeed;
use crate::cluster::renders::{model, STANDBY, WORKER};
use crate::cluster::view::ClusterSheetView;
use crate::mac::shell::AppCluster;

pub fn app_cluster(cx: &Context<ChatView>) -> Arc<ClusterFeed> {
    cx.try_global::<AppCluster>()
        .map(|c| c.0.clone())
        .unwrap_or_else(|| Arc::new(ClusterFeed::none()))
}

impl ChatView {
    pub(super) fn open_cluster_sheet(&mut self, cx: &mut Context<Self>) {
        let feed = self.cluster.clone();
        let entity = cx.new(|cx| ClusterSheetView::live(feed, cx));
        self.show_sheet(entity, cx);
    }

    pub(super) fn cluster_tick(&mut self, cx: &mut Context<Self>) {
        let generation = self.cluster.generation();
        if generation != self.seen_cluster {
            self.seen_cluster = generation;
            self.redraw_chrome(cx);
            cx.notify();
        }
    }

    pub(super) fn nodes(&self) -> Nodes {
        Nodes::from_cluster(&self.cluster.snapshot())
    }

    pub(super) fn offline_note(&self) -> Option<String> {
        reply_note(self.selected.as_deref()?, &self.nodes())
    }

    pub(super) fn preview_remote(&mut self) {
        self.cluster = Arc::new(ClusterFeed::fixed(model(true)));
        self.remote_preview = true;
    }

    pub(super) fn apply_remote_preview(&mut self) {
        if !self.remote_preview {
            return;
        }
        for (session, node) in self.sessions.iter_mut().zip([STANDBY, WORKER]) {
            if Qualified::parse(&session.agent.id).is_some() {
                continue;
            }
            let qualified = format!("{node}/{}", session.agent.id);
            if self.selected.as_deref() == Some(session.agent.id.as_str()) {
                self.selected = Some(qualified.clone());
            }
            session.agent.id = qualified;
        }
        if let Some(second) = self.sessions.get(1) {
            self.selected = Some(second.agent.id.clone());
        }
    }
}
