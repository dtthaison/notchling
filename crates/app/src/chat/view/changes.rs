use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyElement, AnyView, Context, StyleRefinement, Window};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SIDEBAR_WIDTH;
use notchling_kit::ui::{hsla, KitPart, KitStyled};
use notchling_kit::{Appearance, Ink};

use super::page_panel::resize_edge;
use super::render_map::WindowRegion;
use super::ChatView;
use crate::chat::branch::{self, Branches, Summary};
use crate::chat::branch_rows::Slot;
use crate::chat::git::{self, DiffText, Why};
use crate::chat::page_panel::panel_width;

impl ChatView {
    pub(super) fn changes_visible(&self) -> bool {
        self.repo.changes_shown
            && !self.page.is_open()
            && !self.terminal.shown()
            && !self.simulator.shown()
    }

    pub(super) fn changes_summary(&self) -> Option<Summary> {
        self.repo
            .branch_for(self.selected_folder().as_deref())
            .map(|m| m.summary())
    }

    pub(super) fn push_branch(&mut self, cx: &mut Context<Self>) {
        let model = self.repo.branch_for(self.selected_folder().as_deref());
        let again = self
            .changes_panel()
            .update(cx, |panel, pcx| panel.set_model(model, pcx));
        for path in again {
            self.fetch_change_diff(path, cx);
        }
    }

    fn changes_panel(&self) -> gpui::Entity<super::changes_panel::ChangesPanel> {
        self.parts.changes.clone()
    }

    pub(super) fn open_changes(&mut self, cx: &mut Context<Self>) {
        self.repo.changes_shown = true;
        self.push_branch(cx);
        self.git_ask(Why::Opened, cx);
        cx.notify();
    }

    pub(super) fn close_changes(&mut self, cx: &mut Context<Self>) {
        self.repo.changes_shown = false;
        self.changes_panel()
            .update(cx, |panel, pcx| panel.close_menu(pcx));
        self.git_ask(Why::Opened, cx);
        cx.notify();
    }

    pub(super) fn toggle_change_file(&mut self, path: &str, cx: &mut Context<Self>) {
        let opened = self
            .changes_panel()
            .update(cx, |panel, pcx| panel.toggle(path, pcx));
        if opened {
            self.fetch_change_diff(path.to_owned(), cx);
        }
    }

    fn fetch_change_diff(&mut self, path: String, cx: &mut Context<Self>) {
        let model = self.changes_panel().read(cx).model().cloned();
        let (Some(model), folder) = (model, self.selected_folder()) else {
            return;
        };
        if self.root.fixture.is_some() {
            if self.repo.previewing() {
                let slot = Slot::from_diff(&git::preview_diff());
                self.changes_panel().update(cx, |panel, pcx| {
                    panel.set_slot(&path, &model.base, slot, pcx)
                });
            }
            return;
        }
        let Some(folder) = folder else {
            return;
        };
        let from = model.find(&path).and_then(|f| f.from.clone());
        let request = branch::file_request(&folder, &model.base, &path, from.as_deref());
        let base = model.base.clone();
        let acts = self.acts(cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { acts.actions.read(&request) })
                .await;
            let slot = match result.and_then(|reply| {
                git::parse_diff(&reply).ok_or_else(|| "The diff came back unreadable.".to_owned())
            }) {
                Ok(diff) => Slot::from_diff(&diff),
                Err(why) => Slot::Failed(why),
            };
            this.update(cx, |view, cx| {
                view.changes_panel()
                    .update(cx, |panel, pcx| panel.set_slot(&path, &base, slot, pcx));
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn toggle_change_menu(&mut self, cx: &mut Context<Self>) {
        let opened = self
            .changes_panel()
            .update(cx, |panel, pcx| panel.toggle_menu(pcx));
        if !opened {
            return;
        }
        if self.root.fixture.is_some() {
            if self.repo.previewing() {
                let list = Arc::new(branch::preview_branches());
                self.changes_panel()
                    .update(cx, |panel, pcx| panel.set_branches(Some(list), pcx));
            }
            return;
        }
        let Some(folder) = self.selected_folder() else {
            return;
        };
        let acts = self.acts(cx);
        let request = branch::branches_request(&folder);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { acts.actions.read(&request) })
                .await;
            let list: Option<Branches> = result.ok().and_then(|r| branch::parse_branches(&r));
            this.update(cx, |view, cx| {
                let list = list.map(Arc::new);
                view.changes_panel()
                    .update(cx, |panel, pcx| panel.set_branches(list, pcx));
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn close_change_menu(&mut self, cx: &mut Context<Self>) {
        self.changes_panel()
            .update(cx, |panel, pcx| panel.close_menu(pcx));
    }

    pub(super) fn set_change_base(&mut self, base: String, cx: &mut Context<Self>) {
        self.close_change_menu(cx);
        if self.repo.base_choice.as_deref() == Some(base.as_str()) {
            return;
        }
        self.repo.base_choice = Some(base);
        self.repo.gate_reset();
        self.git_ask(Why::Opened, cx);
    }

    pub(super) fn open_branch_diff(
        &mut self,
        path: &str,
        from: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let model = self.changes_panel().read(cx).model().cloned();
        let Some(model) = model else {
            return;
        };
        self.repo.diff_via = Some((model.base.clone(), from.map(str::to_owned)));
        super::select::clear();
        let asked = self.page.show_diff(path);
        if self.root.fixture.is_some() {
            if self.repo.previewing() {
                self.page.set_diff(asked, path, Ok(git::preview_diff()));
            }
        } else if let Some(folder) = self.selected_folder() {
            self.fetch_git_diff(folder, path.to_owned(), asked, cx);
        }
        cx.notify();
    }

    pub fn preview_changes(&mut self, how: &str, cx: &mut Context<Self>) {
        self.repo.show_preview();
        self.repo.changes_shown = true;
        self.push_branch(cx);
        if how == "expanded" {
            let diff: DiffText = git::preview_diff();
            self.changes_panel().update(cx, |panel, pcx| {
                panel.expand(branch::PREVIEW_PATH, Slot::from_diff(&diff), pcx)
            });
        }
        if how == "menu" {
            self.toggle_change_menu(cx);
        }
        self.refresh_details(cx);
    }
}

pub(super) fn render(
    view: &ChatView,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Option<AnyElement> {
    if !view.changes_visible() {
        return None;
    }
    let sidebar = if view.sidebar_hidden {
        0.0
    } else {
        SIDEBAR_WIDTH
    };
    let available = f32::from(window.viewport_size().width) - sidebar;
    let width = panel_width(view.page.width, available);
    view.page.set_shown_width(width);
    let part =
        AnyView::from(view.parts.changes.clone()).cached(StyleRefinement::default().size_full());
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
            .part_at("Changes Panel", Corner::TopTrailing, a, cx)
            .into_any_element(),
    )
}
