//! The menu a right click on text opens, at the pointer: Copy. It works on what is selected in the
//! conversation or the file panel (the click doesn't change the selection, so a selection can be
//! made first and copied after), and shows faint, doing nothing, when nothing is. The reply box
//! has no menu of its own yet.

use gpui::{prelude::*, px, Context, Div, Pixels, Point, SharedString};
use notchling_kit::labels::Corner;
use notchling_kit::ui::{card, KitPart};
use notchling_kit::Appearance;

use super::accounts::{held_item, menu_item};
use super::row_menu::popup;
use super::ChatView;
use crate::acting::Control;

/// An open Copy menu: where the click was, in the window, and whether text was selected then.
#[derive(Clone, Debug, PartialEq)]
pub struct CopyMenu {
    pub at: Point<Pixels>,
    pub enabled: bool,
}

const WIDTH: f32 = 120.0;

/// Whether a selection from `anchor` to `focus` holds anything: both ends are set and differ. A
/// click leaves them equal, and nothing is selected.
pub fn selects<T: PartialEq>(anchor: Option<&T>, focus: Option<&T>) -> bool {
    matches!((anchor, focus), (Some(a), Some(f)) if a != f)
}

/// The open menu over a click-away layer, or nothing.
pub(super) fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let menu = view.copy_menu.as_ref()?;
    let id = SharedString::from("copy-menu-copy");
    let item = if menu.enabled {
        menu_item(id, "Copy", false, a)
            .on_click(cx.listener(|this, _, window, cx| {
                this.close_copy_menu(cx);
                this.copy_selection(window, cx);
            }))
            .into_any_element()
    } else {
        held_item(
            id,
            "Copy",
            Control {
                enabled: false,
                tooltip: Some("Select some text first."),
            },
            a,
        )
        .into_any_element()
    };
    let card = card(a)
        .w(px(WIDTH))
        .p(px(6.0))
        .flex()
        .flex_col()
        .shadow_lg()
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
    /// Opens the menu at `at` on a right click on text; `enabled` is whether a selection was there.
    /// Nothing opens under a sheet or another menu.
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

    /// The Copy menu is open (the file panel's selection listener asks, with no app handle to the
    /// chat's fields).
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
