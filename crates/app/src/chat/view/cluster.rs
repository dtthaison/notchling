use std::sync::Arc;

use gpui::{AppContext, Context};
use notchling_engine::cluster::Qualified;
use notchling_engine::sessions::AgentGroup;
use notchling_locale as t;

use super::ChatView;
use crate::acting::Control;
use crate::acting::Request;
use crate::chat::remote_control::{refused, remote_control, Action};
use crate::chat::sidebar_node::{reply_note, Nodes};
use crate::cluster::feed::ClusterFeed;
use crate::cluster::move_sheet::Names;
use crate::cluster::renders::{model, STANDBY, WORKER};
use crate::cluster::secrets::VaultAccess;
use crate::cluster::view::ClusterSheetView;
use crate::mac::shell::{AppCluster, AppVault};
use crate::phone::bridge::now_ms;
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
        let names: Names = self
            .sessions
            .iter()
            .map(|s| (s.agent.id.clone(), s.agent.name.clone()))
            .collect();
        let access = self.vault_access(cx);
        let entity = cx.new(|cx| ClusterSheetView::live(feed, access, cx).with_names(names));
        self.show_sheet(entity, cx);
    }

    pub(super) fn keep_on_machine(&mut self, row: &str, cx: &mut Context<Self>) {
        self.row_menu = None;
        let Some(request) = self.cluster.snapshot().keep_toggle(row) else {
            cx.notify();
            return;
        };
        let feed = self.cluster.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let result = feed.send(&request);
                    feed.refresh();
                    result
                })
                .await;
            this.update(cx, |view, cx| {
                if let Err(error) = result {
                    view.tools.error = Some(t::cluster::keep_failed(&error));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn vault_access(&self, cx: &Context<Self>) -> VaultAccess {
        VaultAccess {
            local: app_vault(cx),
            switches: self.switches.clone(),
        }
    }

    pub(super) fn cluster_tick(&mut self, cx: &mut Context<Self>) {
        self.follow_remote(cx);
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

    pub(super) fn follow_remote(&mut self, cx: &mut Context<Self>) {
        if self.remote_preview || self.root.fixture.is_some() {
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        let row = session.agent.id.clone();
        let now = now_ms();
        if !self.followed.due(Some(session), now) || self.remote_refused(&row, Action::Reply) {
            return;
        }
        let request = self.followed.begin(&row, now);
        let actions = self.acts(cx).actions;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.read(&Request::cluster_follow(&row)) })
                .await;
            this.update(cx, |view, _| {
                view.followed.finish(request, result.is_ok(), now_ms());
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn cluster_snapshot(&self) -> crate::cluster::model::ClusterModel {
        self.cluster.snapshot()
    }

    pub(super) fn nodes(&self) -> Nodes {
        Nodes::from_cluster(&self.cluster.snapshot())
    }

    pub(super) fn offline_note(&self) -> Option<String> {
        let id = self.selected.as_deref()?;
        Qualified::parse(id)?;
        reply_note(id, &self.nodes())
    }

    pub(super) fn remote_control(&self, id: &str, action: Action) -> Option<Control> {
        remote_control(id, &self.nodes(), action)
    }

    pub(super) fn remote_refused(&self, id: &str, action: Action) -> bool {
        refused(self.remote_control(id, action))
    }

    pub(super) fn selected_refused(&self, action: Action) -> bool {
        self.selected
            .as_deref()
            .is_some_and(|id| self.remote_refused(id, action))
    }

    pub(super) fn attach_refused(&self) -> bool {
        self.selected_session()
            .is_some_and(|s| !crate::chat::remote_control::takes_files(&s.agent))
    }

    pub(super) fn refuse_selected(&mut self, action: Action, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.selected.clone() else {
            return false;
        };
        let Some(control) = self.remote_control(&id, action).filter(|c| !c.enabled) else {
            return false;
        };
        self.tools.error = control
            .tooltip
            .map(str::to_owned)
            .or_else(|| reply_note(&id, &self.nodes()));
        cx.notify();
        true
    }

    pub(super) fn preview_remote(&mut self, shell: bool) {
        self.cluster = Arc::new(ClusterFeed::fixed(model(true)));
        self.remote_preview = true;
        self.remote_shell = shell;
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
                AgentGroup::Working if self.remote_shell => STANDBY,
                AgentGroup::Working => WORKER,
                AgentGroup::NeedsInput => STANDBY,
                AgentGroup::Completed => continue,
            };
            if self.remote_shell && session.agent.group == AgentGroup::Working {
                session.agent.id = format!("{node}/shell:j-1");
                session.agent.name = "make test".to_owned();
                session.agent.command = Some("make test".to_owned());
                session.agent.target = notchling_engine::sessions::AgentTarget::Process(42);
                session.agent.host = "Shell".to_owned();
                session.agent.transcript_path = None;
                target = Some(session.agent.id.clone());
                continue;
            }
            session.agent.id = format!("{node}/{}", session.agent.id);
            if node == WORKER && !self.remote_shell {
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
