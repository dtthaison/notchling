use std::rc::Rc;

use gpui::{div, prelude::*, px, App, ClickEvent, Div, Window};
use notchling_engine::versions::Mark;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, label, KitPart};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::client_model::{ClientPart, WorkerLine};

pub const TITLE: &str = "Client";
pub const CHECKING: &str = "Looking…";
pub const CHECK_AGAIN: &str = "Check Again";
pub const CANCEL: &str = "Cancel";

pub type Click = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(Clone)]
pub struct ClientHandlers {
    pub check: Click,
    pub update: Click,
    pub ask_back: Click,
    pub confirm_back: Click,
    pub cancel_back: Click,
    pub confirming: bool,
}

fn mark_ink(mark: Mark) -> Ink {
    match mark {
        Mark::Same => Ink::Done,
        Mark::Behind | Mark::Ahead => Ink::AccentText,
        Mark::Held | Mark::CantPin | Mark::NotInstalled | Mark::Unknown => Ink::TextMuted,
    }
}

fn row_shell(a: Appearance, first: bool) -> Div {
    let row = div()
        .px(px(14.0))
        .py(px(8.0))
        .min_h(px(24.0))
        .flex()
        .items_center()
        .gap(px(12.0));
    if first {
        row
    } else {
        row.border_t(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
    }
}

fn worker_row(line: &WorkerLine, a: Appearance) -> Div {
    row_shell(a, false)
        .child(label(&line.machine, TypeRole::Body, Ink::Text, a).flex_shrink_0())
        .child(label(&line.text, TypeRole::Meta, Ink::TextMuted, a).flex_1())
        .child(label(line.state, TypeRole::Meta, mark_ink(line.mark), a).flex_shrink_0())
}

fn note_stack(text: &str, ink: Ink, note: Option<&str>, a: Appearance) -> Div {
    let mut stack =
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(label(text, TypeRole::Body, ink, a));
    if let Some(note) = note {
        stack = stack.child(label(note, TypeRole::Meta, Ink::TextMuted, a));
    }
    stack
}

fn action(
    id: &'static str,
    title: &str,
    kind: ButtonKind,
    enabled: bool,
    click: &Click,
    a: Appearance,
) -> gpui::Stateful<Div> {
    let state = if enabled {
        PartState::Rest
    } else {
        PartState::Disabled
    };
    let click = click.clone();
    let shown = button(id, title, kind, state, a).flex_shrink_0();
    if enabled {
        shown.on_click(move |event, window, cx| click(event, window, cx))
    } else {
        shown
    }
}

fn client_rows(part: &ClientPart, handlers: &ClientHandlers, a: Appearance) -> Div {
    let mut rows = div()
        .flex()
        .flex_col()
        .child(row_shell(a, true).child(label(&part.found, TypeRole::Body, Ink::Text, a).flex_1()));
    if let Some(latest) = &part.latest {
        let mut row = row_shell(a, false);
        match &part.update {
            Some(update) => {
                row = row
                    .child(note_stack(latest, Ink::Text, update.note.as_deref(), a))
                    .child(action(
                        "client-update",
                        "Update",
                        ButtonKind::Secondary,
                        update.enabled && !part.busy,
                        &handlers.update,
                        a,
                    ));
            }
            None => {
                row = row.child(note_stack(latest, Ink::TextMuted, None, a));
            }
        }
        rows = rows.child(row);
    }
    if let Some(check) = &part.check {
        let ink = if check.failed { Ink::Needs } else { Ink::Text };
        rows = rows.child(
            row_shell(a, false)
                .child(note_stack(&check.text, ink, check.details.as_deref(), a))
                .child(action(
                    "client-check",
                    CHECK_AGAIN,
                    ButtonKind::Secondary,
                    !part.busy,
                    &handlers.check,
                    a,
                )),
        );
    }
    if let Some((line, red)) = &part.soak {
        let ink = if *red { Ink::Needs } else { Ink::TextMuted };
        rows = rows.child(row_shell(a, false).child(label(line, TypeRole::Meta, ink, a).flex_1()));
    }
    if let Some(back) = &part.go_back {
        let row = if handlers.confirming {
            row_shell(a, false)
                .child(note_stack(&back.confirm, Ink::Text, None, a))
                .child(action(
                    "client-go-back-confirm",
                    &back.label,
                    ButtonKind::Primary,
                    !part.busy,
                    &handlers.confirm_back,
                    a,
                ))
                .child(action(
                    "client-go-back-cancel",
                    CANCEL,
                    ButtonKind::Ghost,
                    true,
                    &handlers.cancel_back,
                    a,
                ))
        } else {
            row_shell(a, false).child(div().flex_1()).child(action(
                "client-go-back",
                &back.label,
                ButtonKind::Ghost,
                !part.busy,
                &handlers.ask_back,
                a,
            ))
        };
        rows = rows.child(row);
    }
    for line in &part.workers {
        rows = rows.child(worker_row(line, a));
    }
    rows
}

pub fn client_section(
    part: Option<&ClientPart>,
    handlers: &ClientHandlers,
    a: Appearance,
    cx: &App,
) -> Div {
    let rows = match part {
        Some(part) => client_rows(part, handlers, a),
        None => div().flex().flex_col().child(
            row_shell(a, true).child(label(CHECKING, TypeRole::Body, Ink::Text, a).flex_1()),
        ),
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(label(TITLE, TypeRole::Section, Ink::TextFaint, a))
        .child(card(a).overflow_hidden().child(rows))
        .part("Client", a, cx)
}
