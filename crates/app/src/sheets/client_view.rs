use gpui::{div, prelude::*, px, App, Div};
use notchling_engine::versions::Mark;
use notchling_kit::ui::{card, hsla, label, KitPart};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::client_model::{ClientPart, WorkerLine};

pub const TITLE: &str = "Client";
pub const CHECKING: &str = "Looking…";

fn mark_ink(mark: Mark) -> Ink {
    match mark {
        Mark::Same => Ink::Done,
        Mark::Behind | Mark::Ahead => Ink::AccentText,
        Mark::Held | Mark::CantPin | Mark::NotInstalled | Mark::Unknown => Ink::TextMuted,
    }
}

fn worker_row(line: &WorkerLine, a: Appearance) -> Div {
    div()
        .px(px(14.0))
        .py(px(8.0))
        .min_h(px(24.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .border_t(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .child(label(&line.machine, TypeRole::Body, Ink::Text, a).flex_shrink_0())
        .child(label(&line.text, TypeRole::Meta, Ink::TextMuted, a).flex_1())
        .child(label(line.state, TypeRole::Meta, mark_ink(line.mark), a).flex_shrink_0())
}

pub fn client_section(part: Option<&ClientPart>, a: Appearance, cx: &App) -> Div {
    let found = part.map_or(CHECKING, |p| p.found.as_str());
    let mut rows = div().flex().flex_col().child(
        div()
            .px(px(14.0))
            .py(px(8.0))
            .min_h(px(24.0))
            .flex()
            .items_center()
            .child(label(found, TypeRole::Body, Ink::Text, a).flex_1()),
    );
    if let Some(part) = part {
        for line in &part.workers {
            rows = rows.child(worker_row(line, a));
        }
    }
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(label(TITLE, TypeRole::Section, Ink::TextFaint, a))
        .child(card(a).overflow_hidden().child(rows))
        .part("Client", a, cx)
}
