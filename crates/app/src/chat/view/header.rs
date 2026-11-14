use gpui::{div, prelude::*, px, Context, Div};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button_with, hsla, icon_button, label, phone_mark, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};
use notchling_locale as t;

use super::phone::{button_tip_off, button_tip_on};
use super::render_map::WindowRegion;
use super::sidebar::{HEADER_BUTTON, HEADER_ROW, HIDDEN_HEADER_START};
use super::ChatView;
use crate::chat::header::header;
use crate::mac::gate;

pub(super) fn chip(text: &str, a: Appearance) -> Div {
    div()
        .h(px(20.0))
        .px(px(8.0))
        .flex_shrink_0()
        .rounded_full()
        .fill(Ink::Raised, a)
        .flex()
        .items_center()
        .child(label(text, TypeRole::Meta, Ink::TextMuted, a))
}

fn phone_button(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> impl IntoElement {
    let on = view.bridge_on();
    let kind = if view.phone.is_some() {
        ButtonKind::Secondary
    } else {
        ButtonKind::Ghost
    };
    let tip = if on {
        button_tip_on()
    } else {
        button_tip_off()
    };
    let ink = if view.phone.is_some() {
        Ink::Text
    } else {
        Ink::TextMuted
    };
    gate::tip(
        button_with(
            "phone-button",
            phone_mark(14.0, ink, a),
            kind,
            PartState::Rest,
            a,
        )
        .size(px(HEADER_BUTTON))
        .px(px(0.0)),
        tip,
        a,
    )
    .relative()
    .when(on, |b| {
        b.child(
            div()
                .absolute()
                .top(px(5.0))
                .right(px(5.0))
                .size(px(6.0))
                .rounded_full()
                .fill(Ink::Done, a),
        )
    })
    .on_click(cx.listener(|this, _, _, cx| this.toggle_phone(cx)))
    .part_at("Phone Button", Corner::BottomTrailing, a, cx)
}

fn details_button(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> impl IntoElement {
    let shown = !view.details_hidden;
    let kind = if shown {
        ButtonKind::Secondary
    } else {
        ButtonKind::Ghost
    };
    let tip = if shown {
        t::chat::window_hide_details()
    } else {
        t::chat::window_show_details()
    };
    gate::tip_text(
        icon_button(
            "details-button",
            Icon::PanelRight,
            IconSize::Button.points(),
            kind,
            PartState::Rest,
            a,
        )
        .size(px(HEADER_BUTTON))
        .px(px(0.0)),
        tip.into(),
        a,
    )
    .on_click(cx.listener(|this, _, _, cx| this.toggle_details(cx)))
    .part_at("Details Button", Corner::BottomTrailing, a, cx)
}

pub fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let bar = div()
        .h(px(HEADER_ROW))
        .flex_shrink_0()
        .px(px(SIDE_PADDING))
        .flex()
        .items_center()
        .gap(px(4.0))
        .border_b(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .when(view.sidebar_hidden, |bar| bar.pl(px(HIDDEN_HEADER_START)));
    let Some(session) = view.selected_session() else {
        return bar
            .child(
                label(
                    t::chat::window_no_session(),
                    TypeRole::Meta,
                    Ink::TextMuted,
                    a,
                )
                .flex_1(),
            )
            .child(details_button(view, a, cx))
            .child(super::terminal::button(view, a, cx))
            .child(super::simulator::button_in_header(view, a, cx))
            .child(phone_button(view, a, cx))
            .window_region_badge(a, cx)
            .part_at("Header", Corner::Bottom, a, cx);
    };
    let h = header(
        session,
        &view.conversation,
        &view.root.home,
        view.claude_catalog(),
    );
    bar.child(
        label(&h.title, TypeRole::Heading, Ink::Text, a)
            .flex_1()
            .min_w(px(0.0))
            .elide()
            .part_at("Session Name", Corner::Trailing, a, cx),
    )
    .child(details_button(view, a, cx))
    .child(super::terminal::button(view, a, cx))
    .child(super::simulator::button_in_header(view, a, cx))
    .child(phone_button(view, a, cx))
    .window_region_badge(a, cx)
    .part_at("Header", Corner::Bottom, a, cx)
}
