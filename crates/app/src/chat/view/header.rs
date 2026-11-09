//! The header over the conversation: avatar, title, folder and host, context, shells, status.

use gpui::{div, prelude::*, px, Context, Div};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{AVATAR, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    avatar, button_with, hsla, icon_button, label, phone_mark, status_pill, KitPart, KitStyled,
};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::accounts;
use super::phone::{BUTTON_TIP_OFF, BUTTON_TIP_ON};
use super::sidebar::HIDDEN_HEADER_START;
use super::ChatView;
use crate::chat::header::header;
use crate::mac::gate;

/// A small raised chip of meta text (the context meter, the shells count).
fn chip(text: &str, a: Appearance) -> Div {
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

/// The Phone button at the right end of the header; a green dot says the bridge is on.
fn phone_button(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> impl IntoElement {
    let on = view.bridge_on();
    let kind = if view.phone.is_some() {
        ButtonKind::Secondary
    } else {
        ButtonKind::Ghost
    };
    let tip = if on { BUTTON_TIP_ON } else { BUTTON_TIP_OFF };
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
        ),
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

/// The Details button, the Swift header's `sidebar.right`: raised while the panel shows.
fn details_button(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> impl IntoElement {
    let shown = !view.details_hidden;
    let kind = if shown {
        ButtonKind::Secondary
    } else {
        ButtonKind::Ghost
    };
    let tip = if shown {
        "Hide the session details"
    } else {
        "Show the session details: branch, model, tokens, tasks, files changed"
    };
    gate::tip_text(
        icon_button(
            "details-button",
            Icon::PanelRight,
            IconSize::Button.points(),
            kind,
            PartState::Rest,
            a,
        ),
        tip.into(),
        a,
    )
    .on_click(cx.listener(|this, _, _, cx| this.toggle_details(cx)))
    .part_at("Details Button", Corner::BottomTrailing, a, cx)
}

pub fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let bar = div()
        .h(px(56.0))
        .flex_shrink_0()
        .px(px(SIDE_PADDING))
        .flex()
        .items_center()
        .gap(px(12.0))
        .border_b(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        // With the sidebar hidden the traffic lights and its button sit over this row's start.
        .when(view.sidebar_hidden, |bar| bar.pl(px(HIDDEN_HEADER_START)));
    let Some(session) = view.selected_session() else {
        return bar
            .child(label("No session selected", TypeRole::Meta, Ink::TextMuted, a).flex_1())
            .child(details_button(view, a, cx))
            .child(phone_button(view, a, cx))
            .part_at("Header", Corner::Bottom, a, cx);
    };
    let h = header(session, &view.conversation, &view.root.home);
    let subtitle = match &h.model {
        Some(model) => format!("{} · {} · {}", h.folder, h.host, model),
        None => format!("{} · {}", h.folder, h.host),
    };
    let meters = accounts::header_meters(view, a);
    bar.child(
        avatar(&h.title, AVATAR, Some(h.status), Ink::Window, a).part_at(
            "Avatar",
            Corner::Bottom,
            a,
            cx,
        ),
    )
    .child(
        // Clipped: when the header is narrow the project line elides first, and the account button
        // never spills over the limit meters beside it.
        div()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                label(&h.title, TypeRole::Heading, Ink::Text, a)
                    .elide()
                    .part_at("Session Name", Corner::Trailing, a, cx),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        label(&subtitle, TypeRole::Meta, Ink::TextMuted, a)
                            .min_w(px(0.0))
                            .elide()
                            .part_at("Project Line", Corner::BottomLeading, a, cx),
                    )
                    .children(
                        accounts::account_control(view, a, cx)
                            .map(|b| b.part_at("Account Menu", Corner::Top, a, cx)),
                    ),
            ),
    )
    .when(!meters.is_empty(), |bar| {
        bar.child(
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .children(meters)
                .part_at("Limit Meters", Corner::Bottom, a, cx),
        )
    })
    .when_some(h.context.as_deref(), |bar, context| {
        bar.child(chip(&format!("Context {context}"), a).part_at(
            "Context Meter",
            Corner::Bottom,
            a,
            cx,
        ))
    })
    .when(h.shells > 0, |bar| {
        let text = if h.shells == 1 {
            "1 shell".to_owned()
        } else {
            format!("{} shells", h.shells)
        };
        bar.child(chip(&text, a).part_at("Shells Chip", Corner::Bottom, a, cx))
    })
    .child(status_pill(h.status, &h.status_word, a).part_at("Status Pill", Corner::Top, a, cx))
    .child(details_button(view, a, cx))
    .child(phone_button(view, a, cx))
    .part_at("Header", Corner::Bottom, a, cx)
}
