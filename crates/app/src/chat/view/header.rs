//! The header over the conversation: avatar, title, folder and host, context, shells, status.

use gpui::{div, prelude::*, px, Div};
use notchling_kit::metric::{AVATAR, SIDE_PADDING};
use notchling_kit::ui::{avatar, hsla, label, status_pill, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::ChatView;
use crate::chat::header::header;

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

pub fn render(view: &ChatView, a: Appearance) -> Div {
    let bar = div()
        .h(px(56.0))
        .flex_shrink_0()
        .px(px(SIDE_PADDING))
        .flex()
        .items_center()
        .gap(px(12.0))
        .border_b(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)));
    let Some(session) = view.selected_session() else {
        return bar.child(label(
            "No session selected",
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        ));
    };
    let h = header(session, &view.conversation, &view.root.home);
    let subtitle = match &h.model {
        Some(model) => format!("{} · {} · {}", h.folder, h.host, model),
        None => format!("{} · {}", h.folder, h.host),
    };
    bar.child(avatar(&h.title, AVATAR, Some(h.status), Ink::Window, a))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(label(&h.title, TypeRole::Heading, Ink::Text, a).truncate())
                .child(label(&subtitle, TypeRole::Meta, Ink::TextMuted, a).truncate()),
        )
        .when_some(h.context.as_deref(), |bar, context| {
            bar.child(chip(&format!("Context {context}"), a))
        })
        .when(h.shells > 0, |bar| {
            let text = if h.shells == 1 {
                "1 shell".to_owned()
            } else {
                format!("{} shells", h.shells)
            };
            bar.child(chip(&text, a))
        })
        .child(status_pill(h.status, &h.status_word, a))
}
