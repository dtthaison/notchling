use gpui::{div, prelude::*, px, Context, Div};
use notchling_kit::metric::{RADIUS_CARD, SPINNER_SMALL};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, label, scrollable, spinner};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::view::MachinesView;

impl MachinesView {
    pub(super) fn diff_overlay(&self, a: Appearance, cx: &mut Context<Self>) -> Option<Div> {
        const LIMIT: usize = 2000;
        let diff = self.model.diff.as_ref()?;
        let done = button(
            "diff-done",
            "Done",
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.model.diff = None;
            cx.notify();
        }));
        let body = match &diff.text {
            None => div()
                .id("diff-lines")
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(spinner("diff-fetching", SPINNER_SMALL, Ink::Working, a))
                        .child(label(
                            "Fetching the branch…",
                            TypeRole::Body,
                            Ink::TextMuted,
                            a,
                        )),
                )
                .into_any_element(),
            Some(text) => {
                let mut lines = div()
                    .id("diff-lines")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .p(px(14.0))
                    .flex()
                    .flex_col();
                let all: Vec<&str> = text.lines().collect();
                for line in all.iter().take(LIMIT) {
                    let ink = if line.starts_with("+++")
                        || line.starts_with("---")
                        || line.starts_with("diff ")
                    {
                        Ink::TextMuted
                    } else if line.starts_with('+') {
                        Ink::Done
                    } else if line.starts_with('-') {
                        Ink::Expired
                    } else if line.starts_with("@@") {
                        Ink::AccentText
                    } else {
                        Ink::Text
                    };
                    lines = lines.child(label(line, TypeRole::Mono, ink, a));
                }
                if all.len() > LIMIT {
                    lines = lines.child(label(
                        &format!("… cut at {LIMIT} lines"),
                        TypeRole::Meta,
                        Ink::TextMuted,
                        a,
                    ));
                }
                scrollable(lines).into_any_element()
            }
        };
        Some(
            div()
                .absolute()
                .size_full()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(notchling_kit::ui::hsla(Ink::Shadow.color(a)))
                .child(
                    card(a)
                        .w(px(760.0))
                        .h(px(520.0))
                        .rounded(px(RADIUS_CARD))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .p(px(14.0))
                                .flex()
                                .items_center()
                                .child(
                                    label(
                                        &format!("Diff · {}", diff.title),
                                        TypeRole::Heading,
                                        Ink::Text,
                                        a,
                                    )
                                    .flex_1(),
                                )
                                .child(done),
                        )
                        .child(body),
                ),
        )
    }
}
