use std::path::PathBuf;

use gpui::{div, prelude::*, Context, Div, Entity};

use super::SettingsView;
use crate::costs::setup_view::SetupView;
use crate::costs::view::{CostsView, OpenSetup};
use crate::sheets::agents::Page;
use crate::sheets::view::Dismiss;

impl SettingsView {
    pub fn with_costs(mut self, view: Entity<CostsView>) -> SettingsView {
        self.costs = Some(view);
        self
    }

    pub fn with_claude_dir(mut self, dir: Option<PathBuf>) -> SettingsView {
        self.claude_dir = dir;
        self
    }

    pub(super) fn show_costs(&mut self, cx: &mut Context<Self>) {
        match &self.costs {
            Some(view) => view.update(cx, |view, cx| view.refresh(cx)),
            None => {
                let actions = self.acts.actions.clone();
                let dir = self.claude_dir.clone();
                let view = cx.new(|cx| CostsView::live(actions, dir, cx));
                cx.subscribe(&view, |this, _, _: &OpenSetup, cx| this.open_setup(cx))
                    .detach();
                self.costs = Some(view);
            }
        }
    }

    fn open_setup(&mut self, cx: &mut Context<Self>) {
        let actions = self.acts.actions.clone();
        let setup = cx.new(|cx| SetupView::live(actions, cx));
        cx.subscribe(&setup, |this, _, _: &Dismiss, cx| this.close_setup(cx))
            .detach();
        self.costs_setup = Some(setup);
        cx.notify();
    }

    fn close_setup(&mut self, cx: &mut Context<Self>) {
        self.costs_setup = None;
        if let Some(view) = &self.costs {
            view.update(cx, |view, cx| view.refresh(cx));
        }
        cx.notify();
    }

    pub(super) fn costs_page(&self) -> Option<Entity<CostsView>> {
        self.costs.clone().filter(|_| self.page == Page::Costs)
    }

    pub(super) fn costs_overlay(&self) -> Option<Div> {
        self.costs_setup
            .clone()
            .map(|setup| div().absolute().size_full().occlude().child(setup))
    }
}
