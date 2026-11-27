use gpui::{prelude::*, ClipboardItem, Context, Div, Pixels, Point, SharedString};
use notchling_kit::labels::Corner;
use notchling_kit::ui::{menu_panel, menu_rule, KitPart};
use notchling_kit::Appearance;
use notchling_locale as t;

use super::accounts::{held_item, menu_item};
use super::row_menu::popup;
use super::ChatView;
use crate::acting::Control;
use crate::chat::link_target::{self, Target};
use crate::chat::remote_control::Action;

#[derive(Clone, Debug, PartialEq)]
pub struct CopyMenu {
    pub at: Point<Pixels>,
    pub enabled: bool,
    pub target: Option<Target>,
}

pub fn selects<T: PartialEq>(anchor: Option<&T>, focus: Option<&T>) -> bool {
    matches!((anchor, focus), (Some(a), Some(f)) if a != f)
}

pub(super) fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let menu = view.copy_menu.as_ref()?;
    let id = SharedString::from("copy-menu-copy");
    let item = if menu.enabled {
        menu_item(id, t::chat::copy(), a)
            .on_click(cx.listener(|this, _, window, cx| {
                this.close_copy_menu(cx);
                this.copy_selection(window, cx);
            }))
            .into_any_element()
    } else {
        held_item(
            id,
            t::chat::copy(),
            Control {
                enabled: false,
                tooltip: Some(t::chat::select_first()),
            },
            a,
        )
        .into_any_element()
    };
    let target = menu.target.as_ref().map(|target| {
        let (id, title) = match target {
            Target::Link(_) => ("copy-menu-copy-link", t::chat::copy_link()),
            Target::Path(_) => ("copy-menu-copy-path", t::chat::copy_path()),
        };
        let text = target.text();
        menu_item(SharedString::from(id), title, a).on_click(cx.listener(move |this, _, _, cx| {
            this.close_copy_menu(cx);
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        }))
    });
    let card = menu_panel(a)
        .children(target)
        .children(menu.target.is_some().then(|| menu_rule(a)))
        .child(item)
        .part_at("Copy Menu", Corner::TopTrailing, a, cx);
    Some(popup(
        "copy-menu-click-away",
        menu.at,
        card,
        ChatView::close_copy_menu,
        cx,
    ))
}

impl ChatView {
    pub(super) fn open_copy_menu(
        &mut self,
        at: Point<Pixels>,
        enabled: bool,
        link: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.sheet.is_some()
            || self.phone.is_some()
            || self.account_menu
            || self.row_menu.is_some()
        {
            return;
        }
        let target = link.and_then(|url| self.link_target(&url));
        self.copy_menu = Some(CopyMenu {
            at,
            enabled,
            target,
        });
        cx.notify();
    }

    pub(super) fn open_copy_menu_for_path(
        &mut self,
        at: Point<Pixels>,
        path: String,
        cx: &mut Context<Self>,
    ) {
        let enabled = self.shown_selection(cx).is_some();
        self.open_copy_menu(at, enabled, Some(path), cx);
    }

    fn link_target(&self, url: &str) -> Option<Target> {
        let folder = if self.selected_refused(Action::Files) {
            String::new()
        } else {
            self.selected_session()
                .map(|s| s.agent.cwd.clone())
                .unwrap_or_default()
        };
        link_target::resolve(url, &folder, super::page_panel::home().as_deref(), |path| {
            path.exists()
        })
    }

    pub(super) fn copy_menu_open(&self) -> bool {
        self.copy_menu.is_some()
    }

    pub(super) fn close_copy_menu(&mut self, cx: &mut Context<Self>) {
        if self.copy_menu.take().is_some() {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_holds_text_only_when_its_ends_differ() {
        assert!(selects(Some(&3), Some(&9)));
        assert!(selects(Some(&9), Some(&3)));
        assert!(!selects(Some(&4), Some(&4)), "a click selects nothing");
        assert!(!selects(None::<&i32>, None));
        assert!(!selects(Some(&1), None));
    }
}
