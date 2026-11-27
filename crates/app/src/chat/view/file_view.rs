use notchling_locale as t;

use gpui::{div, prelude::*, px, AnyElement, SharedString, WeakEntity};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, label, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::markdown::{self, Sel};
use super::page_panel::FileState;
use super::ChatView;
use crate::chat::code_tokens::{segments, Class};
use crate::chat::file_panel::Code;
use crate::chat::jsonl_view::Jsonl;

const LINE: f32 = 18.0;
const GUTTER_GAP: f32 = 12.0;
const DIGIT: f32 = 7.4;

pub fn ink_of(class: Option<Class>) -> Ink {
    match class {
        None => Ink::Text,
        Some(Class::Keyword) => Ink::AccentText,
        Some(Class::Str) => Ink::Done,
        Some(Class::Comment) => Ink::TextFaint,
        Some(Class::Number) => Ink::Needs,
        Some(Class::Key) => Ink::Working,
    }
}

pub fn code(key: &str, code: &Code, a: Appearance) -> AnyElement {
    let digits = code.lines.max(1).to_string().len() as f32;
    let gutter_w = digits * DIGIT;
    let chunks = code.chunks.iter().enumerate().map(|(i, chunk)| {
        let parts = segments(&code.spans, chunk.range.clone());
        div()
            .flex()
            .flex_shrink_0()
            .gap(px(GUTTER_GAP))
            .child(
                div()
                    .w(px(gutter_w))
                    .flex_shrink_0()
                    .text_right()
                    .ink(Ink::TextFaint, a)
                    .child(SharedString::from(chunk.gutter.clone())),
            )
            .child(markdown::colored(
                &format!("{key}-c{i}"),
                &Sel::File,
                &SharedString::from(chunk.text.clone()),
                &parts,
                a,
            ))
    });
    div()
        .id(SharedString::from(format!("{key}-x")))
        .w_full()
        .overflow_x_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .min_w_full()
                .kit_type(TypeRole::Mono)
                .line_height(px(LINE))
                .whitespace_nowrap()
                .ink(Ink::Text, a)
                .children(chunks),
        )
        .into_any_element()
}

pub fn invalid_note(code: &Code, a: Appearance) -> Option<AnyElement> {
    let (line, column) = code.invalid?;
    Some(
        label(
            &t::chat::panel_json_invalid(&line.to_string(), &column.to_string()),
            TypeRole::Meta,
            Ink::Needs,
            a,
        )
        .into_any_element(),
    )
}

pub fn jsonl(
    key: &str,
    file: &FileState,
    j: &Jsonl,
    chat: WeakEntity<ChatView>,
    a: Appearance,
) -> AnyElement {
    let digits = j.rows.last().map_or(1, |r| r.number.to_string().len()) as f32;
    let number_w = digits * DIGIT;
    let top = j.more().then(|| {
        let weak = chat.clone();
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(label(
                &t::chat::panel_jsonl_shown(&j.rows.len().to_string()),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ))
            .child(
                button(
                    "file-panel-load-more",
                    if file.paging() {
                        t::chat::panel_loading()
                    } else {
                        t::chat::panel_load_more()
                    },
                    ButtonKind::Secondary,
                    PartState::Rest,
                    a,
                )
                .on_click(move |_, _, cx| {
                    weak.update(cx, |view, cx| view.load_more_lines(cx)).ok();
                }),
            )
    });
    let rows = j.rows.iter().map(|row| {
        let number = row.number;
        let weak = chat.clone();
        let open = file.opened_row(number);
        let head = div()
            .id(SharedString::from(format!("{key}-row-{number}")))
            .w_full()
            .flex()
            .items_start()
            .gap(px(8.0))
            .py(px(2.0))
            .cursor_pointer()
            .kit_type(TypeRole::Mono)
            .line_height(px(LINE))
            .child(
                div()
                    .w(px(number_w))
                    .flex_shrink_0()
                    .text_right()
                    .ink(Ink::TextFaint, a)
                    .child(if number == 0 {
                        "…".to_owned()
                    } else {
                        number.to_string()
                    }),
            )
            .child(
                div()
                    .w(px(78.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .ink(
                        if row.summary.tag == "invalid" {
                            Ink::Expired
                        } else {
                            Ink::AccentText
                        },
                        a,
                    )
                    .child(row.summary.tag.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .ink(Ink::TextMuted, a)
                    .child(row.summary.preview.clone()),
            )
            .on_click(move |_, _, cx| {
                weak.update(cx, |view, cx| view.toggle_line(number, cx))
                    .ok();
            });
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(head)
            .children(open.map(|code_view| {
                let note = invalid_note(code_view, a);
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .pl(px(number_w + 8.0))
                    .pb(px(6.0))
                    .children(note)
                    .child(code(&format!("{key}-open-{number}"), code_view, a))
                    .children(row.cut.then(|| {
                        label(t::chat::panel_line_cut(), TypeRole::Meta, Ink::TextMuted, a)
                    }))
            }))
    });
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .children(top)
        .children(rows)
        .into_any_element()
}
