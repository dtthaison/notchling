use gpui::{
    div, prelude::*, px, AnyElement, Context, Div, ElementId, EventEmitter, SharedString, Stateful,
    Window,
};
use notchling_kit::metric::{SIDE_PADDING, TITLE_BAR};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{hsla, icon as kit_icon, icon_button, label, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::view::Dismiss;
use crate::mac::gate;

pub fn close_button<V: 'static>(
    id: impl Into<ElementId>,
    a: Appearance,
    cx: &mut Context<V>,
    on_close: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> Stateful<Div> {
    gate::tip_text(
        icon_button(id, Icon::Close, ButtonKind::Ghost, PartState::Rest, a),
        SharedString::from(t::chat::sheet_close()),
        a,
    )
    .on_click(cx.listener(move |this, _, window, cx| on_close(this, window, cx)))
}

pub fn dismiss<V: EventEmitter<Dismiss>>(_: &mut V, _: &mut Window, cx: &mut Context<V>) {
    cx.emit(Dismiss);
}

pub fn sheet_header_closing<V: 'static>(
    glyph: Icon,
    title: &str,
    trailing: Vec<AnyElement>,
    a: Appearance,
    cx: &mut Context<V>,
    on_close: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> Div {
    div()
        .h(px(TITLE_BAR + 6.0))
        .flex_shrink_0()
        .px(px(SIDE_PADDING))
        .flex()
        .items_center()
        .gap(px(10.0))
        .fill(Ink::Card, a)
        .border_b(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .child(kit_icon(
            glyph,
            TypeRole::Heading.spec().size,
            Ink::TextMuted,
            a,
        ))
        .child(label(title, TypeRole::Heading, Ink::Text, a))
        .child(div().flex_1())
        .children(trailing)
        .child(close_button("sheet-close", a, cx, on_close))
}

pub fn sheet_header<V: EventEmitter<Dismiss> + 'static>(
    glyph: Icon,
    title: &str,
    trailing: Vec<AnyElement>,
    a: Appearance,
    cx: &mut Context<V>,
) -> Div {
    sheet_header_closing(glyph, title, trailing, a, cx, dismiss::<V>)
}

pub fn card_title(title: Div, close: Stateful<Div>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(title.flex_1().min_w(px(0.0)))
        .child(close)
}
