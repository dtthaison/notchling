use gpui::{div, prelude::*, px, Div, Stateful};
use notchling_kit::ui::{card, label};
use notchling_kit::{Appearance, Ink, TypeRole};

pub const TITLE: &str = "Activity";

pub const NOTE: &str = "What the daemon refused or failed, what the app and your phone did, and \
                        which account was active, newest first.";

pub fn activity_section(open: Stateful<Div>, a: Appearance) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(label(TITLE, TypeRole::Section, Ink::TextFaint, a))
        .child(
            card(a).flex().flex_col().child(
                div()
                    .px(px(14.0))
                    .py(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .min_h(px(24.0))
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(label("Recent activity", TypeRole::Body, Ink::Text, a).flex_1())
                            .child(open),
                    )
                    .child(label(NOTE, TypeRole::Meta, Ink::TextFaint, a)),
            ),
        )
}
