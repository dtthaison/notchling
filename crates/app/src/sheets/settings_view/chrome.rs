use gpui::{div, prelude::*, px, Context, Div};
use notchling_kit::ui::{card, label, KitPart};
use notchling_kit::{sentence, Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::SettingsView;
use crate::sheets::settings::chrome_row;

const FIELD_ID: usize = 9000;

impl SettingsView {
    pub(super) fn chrome_section(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let values = self.values.clone();
        let row = chrome_row(&move |key: &str| values.get(key).cloned());
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(label(
                t::settings::chrome_title(),
                TypeRole::Section,
                Ink::TextFaint,
                a,
            ))
            .child(
                card(a).child(
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
                                .child(label(row.title, TypeRole::Body, Ink::Text, a).flex_1())
                                .child(self.field(&row, FIELD_ID, a, cx)),
                        )
                        .child(label(
                            &sentence(t::settings::claude_use_chrome_note()),
                            TypeRole::Meta,
                            Ink::TextFaint,
                            a,
                        )),
                ),
            )
            .part("Chrome", a, cx)
    }
}
