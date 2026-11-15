use std::sync::Arc;

use gpui::{AppContext, Context};
use notchling_engine::cluster::Qualified;
use notchling_engine::sessions::AgentGroup;

use super::ChatView;
use crate::acting::Control;
use crate::chat::sidebar_node::{reply_control, reply_note, Nodes};
use crate::cluster::feed::ClusterFeed;
use crate::cluster::renders::{model, STANDBY, WORKER};
use crate::cluster::secrets::VaultAccess;
use crate::cluster::view::ClusterSheetView;
use crate::mac::shell::{AppCluster, AppVault};
use crate::vault::store::Vault;

pub fn app_cluster(cx: &Context<ChatView>) -> Arc<ClusterFeed> {
    cx.try_global::<AppCluster>()
        .map(|c| c.0.clone())
        .unwrap_or_else(|| Arc::new(ClusterFeed::none()))
}

pub fn app_vault<V>(cx: &Context<V>) -> Arc<Vault> {
    cx.try_global::<AppVault>()
        .map(|v| v.0.clone())
        .unwrap_or_else(|| Arc::new(Vault::fixed(Vec::new())))
}

impl ChatView {
    pub(super) fn open_cluster_sheet(&mut self, cx: &mut Context<Self>) {
        let feed = self.cluster.clone();
        let access = self.vault_access(cx);
        let entity = cx.new(|cx| ClusterSheetView::live(feed, access, cx));
        self.show_sheet(entity, cx);
    }

    pub(super) fn vault_access(&self, cx: &Context<Self>) -> VaultAccess {
        VaultAccess {
            local: app_vault(cx),
            switches: self.switches.clone(),
        }
    }

    pub(super) fn cluster_tick(&mut self, cx: &mut Context<Self>) {
        let generation = self.cluster.generation();
        if generation != self.seen_cluster {
            self.seen_cluster = generation;
            let nodes = self.nodes();
            if nodes != self.seen_nodes {
                self.seen_nodes = nodes;
                self.redraw_chrome(cx);
                cx.notify();
            }
        }
    }

    pub(super) fn nodes(&self) -> Nodes {
        Nodes::from_cluster(&self.cluster.snapshot())
    }

    pub(super) fn offline_note(&self) -> Option<String> {
        let id = self.selected.as_deref()?;
        Qualified::parse(id)?;
        reply_note(id, &self.nodes())
    }

    pub(super) fn remote_control(&self, id: &str) -> Option<Control> {
        Qualified::parse(id)?;
        reply_control(id, &self.nodes())
    }

    pub(super) fn preview_remote(&mut self) {
        self.cluster = Arc::new(ClusterFeed::fixed(model(true)));
        self.remote_preview = true;
    }

    pub(super) fn apply_remote_preview(&mut self, cx: &mut Context<Self>) {
        if !self.remote_preview {
            return;
        }
        let mut target = None;
        for session in self.sessions.iter_mut() {
            if Qualified::parse(&session.agent.id).is_some() {
                continue;
            }
            let node = match session.agent.group {
                AgentGroup::Working => WORKER,
                AgentGroup::NeedsInput => STANDBY,
                AgentGroup::Completed => continue,
            };
            session.agent.id = format!("{node}/{}", session.agent.id);
            if node == WORKER {
                target = Some(session.agent.id.clone());
            }
        }
        if let Some(id) = target {
            self.select(&id, cx);
        }
    }

    pub(super) fn loader_id(&self, id: Option<&str>) -> Option<String> {
        let id = id?;
        match Qualified::parse(id) {
            Some(q) if self.remote_preview => Some(q.id),
            _ => Some(id.to_owned()),
        }
    }
}
