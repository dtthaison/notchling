use gpui::{prelude::*, Context, Div, Pixels, Point, SharedString};
use notchling_kit::labels::Corner;
use notchling_kit::ui::{menu_panel, KitPart};
use notchling_kit::Appearance;
use notchling_locale as t;

use super::accounts::{held_item, menu_item};
use super::row_menu::popup;
use super::ChatView;
use crate::acting::Control;

#[derive(Clone, Debug, PartialEq)]
pub struct CopyMenu {
    pub at: Point<Pixels>,
    pub enabled: bool,
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
    let card = menu_panel(a)
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
        cx: &mut Context<Self>,
    ) {
        if self.sheet.is_some()
            || self.phone.is_some()
            || self.account_menu
            || self.row_menu.is_some()
        {
            return;
        }
        self.copy_menu = Some(CopyMenu { at, enabled });
        cx.notify();
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
