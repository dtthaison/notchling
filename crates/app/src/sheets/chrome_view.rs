use gpui::{div, prelude::*, px, Context, Div, EventEmitter, Render, SharedString, Window};
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, label, scrollable, window_appearance, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::view::{sheet_header, Dismiss};
use crate::chat::chrome::{Act, Sheet};

pub struct ChromeChoice(pub Act);

pub struct ChromeView {
    sheet: Sheet,
}

impl EventEmitter<Dismiss> for ChromeView {}
impl EventEmitter<ChromeChoice> for ChromeView {}

impl ChromeView {
    pub fn new(sheet: Sheet) -> ChromeView {
        ChromeView { sheet }
    }

    pub fn sheet(&self) -> &Sheet {
        &self.sheet
    }

    fn status_row(&self, a: Appearance) -> Div {
        let ink = if self.sheet.need.is_some() {
            Ink::Needs
        } else {
            Ink::Text
        };
        card(a)
            .px(px(12.0))
            .py(px(10.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(label(
                t::chat::chrome_status(),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ))
            .child(div().flex_1())
            .child(label(self.sheet.status(), TypeRole::Body, ink, a))
    }

    fn problem(&self, a: Appearance) -> Option<Div> {
        let text = self.sheet.problem()?;
        Some(
            card(a)
                .px(px(12.0))
                .py(px(10.0))
                .child(label(&text, TypeRole::Body, Ink::Text, a)),
        )
    }

    fn help(&self, a: Appearance) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(label(
                t::chat::chrome_help_title(),
                TypeRole::Section,
                Ink::TextFaint,
                a,
            ))
            .children(
                self.sheet
                    .help()
                    .into_iter()
                    .map(|line| label(&format!("· {line}"), TypeRole::Meta, Ink::TextMuted, a)),
            )
    }

    fn footer(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let acts = self.sheet.acts();
        div()
            .flex_shrink_0()
            .px(px(SIDE_PADDING))
            .py(px(12.0))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.0))
            .border_t(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .children(acts.into_iter().enumerate().map(|(i, act)| {
                let kind = if i == 0 {
                    ButtonKind::Primary
                } else {
                    ButtonKind::Secondary
                };
                button(
                    SharedString::from(format!("chrome-act-{i}")),
                    act.label(),
                    kind,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(ChromeChoice(act))))
            }))
    }
}

impl Render for ChromeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let close = button(
            "close",
            t::chat::sheet_close(),
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|_, _, _, cx| cx.emit(Dismiss)));
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .overflow_hidden()
            .fill(Ink::Window, a)
            .child(sheet_header(Icon::Web, t::chat::chrome_title(), close, a))
            .child(
                div().flex_1().min_h(px(0.0)).child(scrollable(
                    div()
                        .id("chrome-body")
                        .p(px(SIDE_PADDING))
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .child(self.status_row(a))
                        .children(self.problem(a))
                        .child(self.help(a)),
                )),
            )
            .child(self.footer(a, cx))
    }
}
