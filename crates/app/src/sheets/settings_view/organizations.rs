use gpui::{AppContext, Context, Entity};

use super::SettingsView;
use crate::organizations::view::OrganizationsView;
use crate::sheets::agents::Page;

impl SettingsView {
    pub fn with_organizations(mut self, view: Entity<OrganizationsView>) -> SettingsView {
        self.organizations = Some(view);
        self
    }

    pub(super) fn show_organizations(&mut self, cx: &mut Context<Self>) {
        match &self.organizations {
            Some(view) => view.update(cx, |view, cx| view.refresh(cx)),
            None => {
                let actions = self.acts.actions.clone();
                self.organizations = Some(cx.new(|cx| OrganizationsView::live(actions, cx)));
            }
        }
    }

    pub(super) fn organizations_page(&self) -> Option<Entity<OrganizationsView>> {
        self.organizations
            .clone()
            .filter(|_| self.page == Page::Organizations)
    }
}
