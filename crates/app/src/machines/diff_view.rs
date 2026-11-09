//! The diff sheet of a ready branch (Swift `MachineDiffView`): the text as `git diff` printed
//! it, colored by line, fetched from the machine once the sheet opens.

use gpui::{div, prelude::*, px, Context, Div};
use notchling_kit::metric::RADIUS_CARD;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, label, scrollable};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::view::MachinesView;

impl MachinesView {
    /// The diff sheet of a ready branch: the text as `git diff` printed it, colored by line.
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
                .child(label(
                    "Fetching the branch…",
                    TypeRole::Body,
                    Ink::TextMuted,
                    a,
                ))
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
                // The backdrop takes every click and hover: no card button under it is live.
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
