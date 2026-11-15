use std::sync::Arc;

use gpui::{AppContext, Context};
use notchling_engine::cluster::Qualified;
use notchling_engine::sessions::AgentGroup;

use super::ChatView;
use crate::acting::Control;
use crate::acting::Request;
use crate::chat::remote::follow_due;
use crate::chat::remote_control::{refused, remote_control, Action};
use crate::chat::sidebar_node::{reply_note, Nodes};
use crate::cluster::feed::ClusterFeed;
use crate::cluster::renders::{model, STANDBY, WORKER};
use crate::cluster::view::ClusterSheetView;
use crate::mac::shell::AppCluster;
use crate::phone::bridge::now_ms;

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
        let last = match &self.followed {
            Some((followed, at)) if *followed == row => *at,
            _ => 0,
        };
        let now = now_ms();
        if !follow_due(Some(session), last, now)
            || self.remote_refused(&row, Action::Reply)
            || !self.acting(cx).control().enabled
        {
            return;
        }
        self.followed = Some((row.clone(), now));
        let actions = self.acts(cx).actions;
        cx.background_executor()
            .spawn(async move {
                actions.send(&Request::cluster_follow(&row)).ok();
            })
            .detach();
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
