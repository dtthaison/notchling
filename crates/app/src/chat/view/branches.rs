use std::path::Path;
use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyElement, AnyView, Context, StyleRefinement, Window};
use notchling_kit::labels::Corner;
use notchling_kit::ui::{hsla, KitPart, KitStyled};
use notchling_kit::{Appearance, Ink};
use serde_json::Value;

use super::page_panel::resize_edge;
use super::render_map::WindowRegion;
use super::{now_ms, ChatView};
use crate::chat::branch_list::{self, PanelInput};
use crate::chat::branches_preview;
use crate::chat::branches_state::{hidden_switch, on_switch, Applied, Received, Switch};
use crate::chat::changes_target::Target;
use crate::chat::git::Why;
use crate::chat::page_panel::panel_width;
use crate::chat::panels::after_close_changes;
use crate::chat::worktrees::{self, Open};

const MINUTE_MS: i64 = 60_000;

impl ChatView {
    pub(super) fn branches_visible(&self) -> bool {
        self.repo.branches_shown
            && !self.repo.changes_shown
            && !self.page.is_open()
            && !self.terminal.shown()
            && !self.simulator.shown()
    }

    pub(super) fn open_branches(&mut self, cx: &mut Context<Self>) {
        self.repo.branches_shown = true;
        self.branches_ask(Why::Opened, cx);
        self.push_branches(cx);
        cx.notify();
    }

    pub(super) fn close_branches(&mut self, cx: &mut Context<Self>) {
        self.repo.branches_shown = false;
        self.git_ask(Why::Opened, cx);
        cx.notify();
    }

    pub(super) fn back_to_branches(&mut self, back: bool) {
        self.repo.branches_shown = after_close_changes(self.repo.branches_shown, back);
    }

    pub(super) fn branches_ask(&mut self, why: Why, cx: &mut Context<Self>) {
        if self.root.fixture.is_some() {
            return;
        }
        let visible = self.branches_visible();
        if visible && why == Why::Tick {
            self.push_branches(cx);
        }
        let folder = self.selected_folder();
        if !self
            .repo
            .branches
            .ask(why, folder.as_deref(), visible, now_ms())
        {
            return;
        }
        let Some(folder) = folder else {
            return;
        };
        let generation = self.repo.branches.generation();
        let request = branch_list::branch_list_request(&folder);
        let acts = self.acts(cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { acts.actions.read(&request) })
                .await;
            this.update(cx, |view, cx| {
                view.branches_in(generation, folder, result, cx)
            })
            .ok();
        })
        .detach();
    }

    fn branches_in(
        &mut self,
        generation: u64,
        folder: String,
        result: Result<Value, String>,
        cx: &mut Context<Self>,
    ) {
        let selected = self.selected_folder();
        let received =
            self.repo
                .branches
                .receive(generation, &folder, selected.as_deref(), result, now_ms());
        match received {
            Received::Stale => {}
            Received::Moved => self.branches_ask(Why::Folder, cx),
            Received::Applied(Applied::Same) => {}
            Received::Applied(Applied::Changed) => self.push_branches(cx),
            Received::Applied(Applied::NotRepo) => self.close_branches(cx),
        }
    }

    pub(super) fn branches_after_load(&mut self, cx: &mut Context<Self>) {
        if self.branches_visible() {
            self.push_branches(cx);
        }
    }

    pub(super) fn branches_switched(&mut self, before: Option<String>, cx: &mut Context<Self>) {
        if self.root.fixture.is_some() {
            return;
        }
        let after = self.selected_folder();
        let panel = self.parts.branches.clone();
        if !self.repo.branches_shown {
            if hidden_switch(before.as_deref(), after.as_deref(), &mut self.repo.branches) {
                panel.update(cx, |panel, pcx| panel.switched(pcx));
            }
            return;
        }
        match on_switch(before.as_deref(), after.as_deref()) {
            Switch::Same => panel.update(cx, |panel, pcx| panel.reset_view(pcx)),
            Switch::Reload => {
                self.repo.branches.forget();
                panel.update(cx, |panel, pcx| panel.switched(pcx));
                self.branches_ask(Why::Opened, cx);
            }
            Switch::Close => self.close_branches(cx),
        }
    }

    pub(super) fn push_branches(&mut self, cx: &mut Context<Self>) {
        if self.root.fixture.is_some() {
            return;
        }
        let input = Arc::new(self.branches_input());
        self.parts
            .branches
            .update(cx, |panel, pcx| panel.set_input(input, pcx));
    }

    fn branches_input(&self) -> PanelInput {
        let folder = self.selected_folder();
        let list = self.repo.branches.list_for(folder.as_deref());
        let paths: Vec<(&str, &str)> = list
            .iter()
            .flat_map(|list| list.branches.iter())
            .filter_map(|b| b.worktree.as_ref())
            .map(|w| (w.path.as_str(), w.real.as_str()))
            .collect();
        let held = worktrees::occupancy(&paths, &self.sessions, &self.subagent_rows);
        let mut session_names = std::collections::HashMap::new();
        for open in held.iter().flatten().map(branch_list::held_open) {
            if let Some(name) = self.open_name(open) {
                session_names.insert(branch_list::open_id(open).to_owned(), name);
            }
        }
        let occupancy = paths
            .iter()
            .zip(held)
            .map(|((path, _), held)| ((*path).to_owned(), held))
            .collect();
        PanelInput {
            list,
            failed: self.repo.branches.failed_for(folder.as_deref()),
            occupancy,
            session_names,
            minute_ms: now_ms() / MINUTE_MS * MINUTE_MS,
        }
    }

    fn open_name(&self, open: &Open) -> Option<String> {
        match open {
            Open::Session(id) => self
                .sessions
                .iter()
                .find(|s| s.agent.id == *id)
                .map(|s| s.agent.name.clone()),
            Open::Subagent(id) => self
                .subagent_rows
                .iter()
                .find(|row| row.id == *id)
                .map(|row| row.description.clone()),
        }
    }

    pub(super) fn branch_changes(&mut self, target: Target, cx: &mut Context<Self>) {
        self.open_changes_for(target, true, cx);
    }

    pub(super) fn branch_open(&mut self, open: Open, cx: &mut Context<Self>) {
        match open {
            Open::Session(id) => self.select(&id, cx),
            Open::Subagent(id) => self.open_subagent(&id, cx),
        }
    }

    pub(super) fn branch_reveal(&mut self, path: &str, cx: &mut Context<Self>) {
        cx.reveal_path(Path::new(path));
    }

    pub fn preview_branches(&mut self, state: &str, cx: &mut Context<Self>) {
        self.repo.show_preview();
        self.repo.branches_shown = true;
        let input = Arc::new(branches_preview::preview(state));
        let tab = branches_preview::preview_tab(state);
        let open = branches_preview::preview_open(state);
        self.parts.branches.update(cx, |panel, pcx| {
            panel.set_input(input, pcx);
            panel.set_tab(tab, pcx);
            if let Some(key) = open {
                panel.open_row(key, pcx);
            }
        });
        self.refresh_details(cx);
    }
}

pub(super) fn render(
    view: &ChatView,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Option<AnyElement> {
    if !view.branches_visible() {
        return None;
    }
    let sidebar = view.sidebar_width();
    let available = f32::from(window.viewport_size().width) - sidebar;
    let width = panel_width(view.page.width, available);
    view.page.set_shown_width(width);
    let part =
        AnyView::from(view.parts.branches.clone()).cached(StyleRefinement::default().size_full());
    Some(
        div()
            .relative()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .fill(Ink::Window, a)
            .border_l(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(part)
            .child(resize_edge())
            .window_region(a, cx)
            .part_at("Branches Panel", Corner::TopTrailing, a, cx)
            .into_any_element(),
    )
}
