//! The menu a right click on a sidebar row opens, at the pointer: Copy Session ID for now.

use gpui::{
    div, prelude::*, px, ClipboardItem, Context, Div, MouseButton, MouseDownEvent, Pixels, Point,
    SharedString, Stateful,
};
use notchling_kit::metric::RADIUS_MD;
use notchling_kit::ui::card;
use notchling_kit::Appearance;

use super::accounts::{held_item, menu_item};
use super::ChatView;
use crate::acting::Control;
use crate::chat::sidebar::copyable_id;

/// An open row menu: the row it is for and where the click was, in the window.
#[derive(Clone, Debug)]
pub struct RowMenu {
    pub row_id: String,
    pub at: Point<Pixels>,
}

const WIDTH: f32 = 200.0;

/// Opens the menu on a right click on `row_id`'s row.
pub(super) fn on_right_click(
    row: Stateful<Div>,
    row_id: String,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    row.on_mouse_down(
        MouseButton::Right,
        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
            this.row_menu = Some(RowMenu {
                row_id: row_id.clone(),
                at: event.position,
            });
            cx.stop_propagation();
            cx.notify();
        }),
    )
}

/// The open menu over a click-away layer, or nothing.
pub(super) fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let menu = view.row_menu.as_ref()?;
    let id = copyable_id(&menu.row_id).map(str::to_owned);
    let mut items = card(a)
        .w(px(WIDTH))
        .p(px(6.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .shadow_lg();
    items = match id {
        Some(id) => items.child(
            menu_item(
                SharedString::from("row-copy-id"),
                "Copy Session ID",
                false,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(id.clone()));
                this.row_menu = None;
                cx.notify();
            })),
        ),
        // A draft has no id yet: the item shows, faint, and does nothing.
        None => items.child(held_item(
            SharedString::from("row-copy-id"),
            "Copy Session ID",
            Control {
                enabled: false,
                tooltip: Some("This session hasn't started yet, so it has no id."),
            },
            a,
        )),
    };
    let close = |this: &mut ChatView, cx: &mut Context<ChatView>| {
        this.row_menu = None;
        cx.notify();
    };
    Some(
        div()
            .absolute()
            .size_full()
            .child(
                div()
                    .id("row-menu-click-away")
                    .absolute()
                    .size_full()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| close(this, cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, _, cx| close(this, cx)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(menu.at.x)
                    .top(menu.at.y)
                    .occlude()
                    .rounded(px(RADIUS_MD))
                    .child(items),
            ),
    )
}
