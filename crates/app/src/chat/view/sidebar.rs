//! The sidebar: sessions in their groups, the selected one highlighted; a click selects.

use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_kit::metric::{SIDEBAR_WIDTH, SIDE_PADDING, TITLE_BAR};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, label, session_row, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::ChatView;
use crate::chat::data::Tool;
use crate::chat::sidebar::sections;

/// The footer, with what there is to say about the daemon (down, or not trusted).
fn footer(a: Appearance, note: Option<&'static str>) -> Div {
    div()
        .h(px(44.0))
        .flex_shrink_0()
        .px(px(12.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(button(
            "settings",
            "Settings",
            ButtonKind::Ghost,
            PartState::Disabled,
            a,
        ))
        .child(button(
            "parts",
            "?",
            ButtonKind::Ghost,
            PartState::Disabled,
            a,
        ))
        .children(note.map(|note| label(note, TypeRole::Meta, Ink::TextFaint, a)))
}

pub fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let sections = sections(&view.sessions, view.now_ms);
    let count = view.sessions.len();
    let mut list = div()
        .id("sessions")
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .px(px(8.0))
        .flex()
        .flex_col()
        .gap(px(4.0));
    if sections.is_empty() {
        list = list.child(div().px(px(8.0)).child(label(
            "No sessions in the last day",
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        )));
    }
    for section in sections {
        list = list.child(div().px(px(8.0)).pt(px(12.0)).pb(px(4.0)).child(label(
            section.title,
            TypeRole::Section,
            Ink::TextFaint,
            a,
        )));
        for row in section.rows {
            let id = row.id.clone();
            let state = if view.selected.as_deref() == Some(row.id.as_str()) {
                PartState::Selected
            } else {
                PartState::Rest
            };
            let status_line = match row.tool {
                Tool::Codex => format!("Codex · {}", row.status_line),
                Tool::Claude => row.status_line.clone(),
            };
            list = list.child(
                session_row(
                    SharedString::from(format!("session-{}", row.id)),
                    &row.name,
                    &status_line,
                    &row.age,
                    row.status,
                    state,
                    false,
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.select(&id, cx))),
            );
        }
    }
    div()
        .w(px(SIDEBAR_WIDTH))
        .h_full()
        .flex_shrink_0()
        .fill(Ink::Sidebar, a)
        .border_r(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .flex()
        .flex_col()
        // The traffic lights sit here; nothing else does.
        .child(div().h(px(TITLE_BAR)).flex_shrink_0())
        .child(
            div()
                .px(px(SIDE_PADDING))
                .pb(px(4.0))
                .flex()
                .items_center()
                .justify_between()
                .child(label("Sessions", TypeRole::Title, Ink::Text, a))
                .child(label(&count.to_string(), TypeRole::Meta, Ink::TextFaint, a)),
        )
        .child(list)
        .child(footer(a, view.daemon_note))
}
