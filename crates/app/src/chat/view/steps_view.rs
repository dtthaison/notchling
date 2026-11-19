use notchling_locale as t;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use gpui::{div, prelude::*, px, Context, Div, FontWeight, SharedString, Stateful};
use notchling_engine::transcript::model::{DiffKind, DiffLine, ToolImage, ToolKind};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{RADIUS_MD, RADIUS_SM};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, icon, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::conversation::chevron;
use super::conversation_view::{ConversationInput, ConversationState, ConversationView};
use super::markdown::{self, Sel};
use crate::chat::browser_rows::BrowserRow;
use crate::chat::rows::ToolRow;
use crate::chat::steps::{self, Detail};

pub(super) fn hairline(a: Appearance) -> Div {
    div().flex_1().h(px(1.0)).fill(Ink::Hairline, a)
}

fn diff_line(line: &DiffLine, a: Appearance) -> Div {
    if line.kind == DiffKind::Hunk {
        return div().px(px(10.0)).py(px(3.0)).child(label(
            &line.text,
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ));
    }
    let (ink, sign) = match line.kind {
        DiffKind::Add => (Ink::Done, "+"),
        DiffKind::Remove => (Ink::Expired, "−"),
        _ => (Ink::TextFaint, " "),
    };
    let wash = matches!(line.kind, DiffKind::Add | DiffKind::Remove);
    div()
        .w_full()
        .flex()
        .py(px(1.0))
        .kit_type(TypeRole::Mono)
        .when(wash, |d| d.bg(hsla(ink.color(a).with_alpha(0.12))))
        .child(
            div()
                .w(px(40.0))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .ink(Ink::TextFaint, a)
                .child(line.number.map(|n| n.to_string()).unwrap_or_default()),
        )
        .child(
            div()
                .w(px(20.0))
                .flex_shrink_0()
                .flex()
                .justify_center()
                .font_weight(FontWeight::BOLD)
                .ink(ink, a)
                .child(sign),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .pr(px(10.0))
                .ink(Ink::Text, a)
                .child(if line.text.is_empty() {
                    " ".to_owned()
                } else {
                    line.text.clone()
                }),
        )
}

fn diff_view(
    step: &ToolRow,
    reveal_ok: bool,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> Div {
    let path = Path::new(&step.input);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| step.input.clone());
    let folder = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let reveal = step.input.clone();
    div()
        .w_full()
        .rounded(px(RADIUS_MD))
        .fill(Ink::Card, a)
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(28.0))
                .pl(px(10.0))
                .pr(px(4.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .border_b(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
                .child(icon(Icon::File, 10.0, Ink::Text, a))
                .child(label(&name, TypeRole::Label, Ink::Text, a))
                .child(
                    label(&folder, TypeRole::Meta, Ink::TextFaint, a)
                        .flex_1()
                        .min_w(px(0.0))
                        .elide(),
                )
                .children(reveal_ok.then(|| {
                    button(
                        SharedString::from(format!("reveal-{}", step.id)),
                        t::chat::show_in_finder(),
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.reveal_path(Path::new(&reveal));
                    }))
                })),
        )
        .child(
            div()
                .py(px(4.0))
                .flex()
                .flex_col()
                .children(step.diff.iter().map(|line| diff_line(line, a))),
        )
}

const SHOT_HEIGHT: f32 = 120.0;
const DECODED_KEEP: usize = 24;

type Decoded = (Arc<gpui::Image>, (u32, u32));

thread_local! {
    static DECODED: RefCell<HashMap<String, Decoded>> = Default::default();
}

fn image_format(media_type: &str) -> gpui::ImageFormat {
    match media_type {
        "image/jpeg" | "image/jpg" => gpui::ImageFormat::Jpeg,
        "image/webp" => gpui::ImageFormat::Webp,
        "image/gif" => gpui::ImageFormat::Gif,
        _ => gpui::ImageFormat::Png,
    }
}

fn decoded(key: &str, image: &ToolImage) -> Option<Decoded> {
    if let Some(known) = DECODED.with(|d| d.borrow().get(key).cloned()) {
        return Some(known);
    }
    let bytes = image.bytes()?;
    let head = &bytes[..bytes.len().min(64 * 1024)];
    let shape = crate::chat::image_size::size(head).unwrap_or((4, 3));
    let made = (
        Arc::new(gpui::Image::from_bytes(
            image_format(&image.media_type),
            bytes,
        )),
        shape,
    );
    DECODED.with(|d| {
        let mut d = d.borrow_mut();
        if d.len() >= DECODED_KEEP {
            d.clear();
        }
        d.insert(key.to_owned(), made.clone());
    });
    Some(made)
}

fn shot(key: &str, image: &ToolImage, a: Appearance) -> Option<Div> {
    let (source, (w, h)) = decoded(key, image)?;
    let width = (SHOT_HEIGHT * w as f32 / h.max(1) as f32).clamp(60.0, 320.0);
    Some(
        div()
            .w(px(width))
            .h(px(SHOT_HEIGHT))
            .rounded(px(RADIUS_MD))
            .border(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .overflow_hidden()
            .child(
                gpui::img(source)
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover),
            ),
    )
}

fn shots_view(id: &str, browser: &BrowserRow, a: Appearance) -> Option<Div> {
    if browser.shots.is_empty() && browser.missing == 0 {
        return None;
    }
    Some(
        div()
            .pl(px(24.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .when(!browser.shots.is_empty(), |d| {
                d.child(
                    div().flex().flex_wrap().gap(px(6.0)).children(
                        browser
                            .shots
                            .iter()
                            .enumerate()
                            .filter_map(|(n, image)| shot(&format!("{id}-{n}"), image, a)),
                    ),
                )
            })
            .when(browser.missing > 0, |d| {
                d.child(label(
                    t::chat::browser_shot_missing(),
                    TypeRole::Meta,
                    Ink::TextFaint,
                    a,
                ))
            }),
    )
}

fn browser_detail(browser: &BrowserRow, a: Appearance) -> Vec<Div> {
    let mut parts = Vec::new();
    if !browser.steps.is_empty() {
        parts.push(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .children(browser.steps.iter().map(|title| {
                    label(title, TypeRole::Meta, Ink::TextMuted, a)
                        .min_w(px(0.0))
                        .elide()
                })),
        );
    }
    if let Some(fault) = &browser.fault {
        parts.push(div().child(label(&fault.hint, TypeRole::Meta, Ink::TextFaint, a)));
    }
    parts
}

fn step_detail(
    step: &ToolRow,
    reveal_ok: bool,
    sel: &Sel,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> Option<Div> {
    let mut detail = div().pl(px(24.0)).flex().flex_col().gap(px(6.0));
    match steps::detail(step) {
        Detail::None => return None,
        Detail::Diff => detail = detail.child(diff_view(step, reveal_ok, a, cx)),
        Detail::Browser => {
            if let Some(browser) = &step.browser {
                detail = detail.children(browser_detail(browser, a));
            }
        }
        Detail::Text { input_title, .. } => {
            if let (Some(title), Some(text)) = (input_title, steps::input_text(step)) {
                detail = detail.child(markdown::mono_box(
                    &format!("{}-in", step.id),
                    sel,
                    title,
                    &text,
                    false,
                    a,
                ));
            }
            if let Some(text) = steps::output_text(step) {
                let title = if step.is_error {
                    t::chat::error()
                } else {
                    t::chat::output()
                };
                detail = detail.child(markdown::mono_box(
                    &format!("{}-out", step.id),
                    sel,
                    title,
                    &text,
                    step.is_error,
                    a,
                ));
            }
        }
    }
    Some(detail)
}

fn step_row(
    input: &ConversationInput,
    s: &ConversationState,
    sel: &Sel,
    step: &ToolRow,
    live: bool,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> Div {
    let has_detail = steps::detail(step) != Detail::None;
    let open = has_detail && s.is_open(&step.id);
    let running = live && !step.finished;
    let icon_ink = if step.is_error {
        Ink::Expired
    } else {
        Ink::TextMuted
    };
    let id = step.id.clone();
    let subagent = (step.kind == ToolKind::Agent)
        .then(|| crate::chat::subagent_view::link_for_step(&input.subagents, &step.id))
        .flatten()
        .map(str::to_owned);
    let line = div()
        .id(SharedString::from(format!("step-{}", step.id)))
        .w_full()
        .min_h(px(28.0))
        .px(px(8.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .rounded(px(RADIUS_SM))
        .when(has_detail, |d| {
            d.cursor_pointer()
                .hover(|s| s.bg(hsla(Ink::Raised.color(a).with_alpha(0.6))))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(&id, cx)))
        })
        .child(
            div()
                .w(px(16.0))
                .flex_shrink_0()
                .flex()
                .justify_center()
                .child(icon(step.icon, 11.0, icon_ink, a)),
        )
        .child(
            label(&step.title, TypeRole::Body, Ink::Text, a)
                .flex_1()
                .min_w(px(0.0))
                .elide(),
        )
        .children(subagent.map(|agent_id| {
            div()
                .id(SharedString::from(format!("step-open-{}", step.id)))
                .flex_shrink_0()
                .cursor_pointer()
                .child(label(t::chat::open(), TypeRole::Label, Ink::AccentText, a))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.act(|chat, cx| chat.open_subagent(&agent_id, cx), cx);
                }))
        }))
        .when(step.added + step.removed > 0, |d| {
            d.child(label(
                &format!("+{}", step.added),
                TypeRole::Mono,
                Ink::Done,
                a,
            ))
            .child(label(
                &format!("−{}", step.removed),
                TypeRole::Mono,
                Ink::Expired,
                a,
            ))
        })
        .when(!running && step.is_error, |d| {
            d.child(icon(Icon::Failed, 11.0, Ink::Expired, a))
        })
        .when(has_detail, |d| d.child(chevron(open, 9.0, a)))
        .part_at("Step Row", Corner::Trailing, a, cx);
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(line)
        .children(
            step.browser
                .as_ref()
                .and_then(|b| shots_view(&step.id, b, a)),
        )
        .when(open, |d| {
            d.children(step_detail(step, input.can_reveal, sel, a, cx))
        })
}

fn step_chip(
    id: &str,
    steps: &[ToolRow],
    open: bool,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> Stateful<Div> {
    let icons =
        div()
            .flex()
            .items_center()
            .children(
                steps::icons(steps)
                    .into_iter()
                    .enumerate()
                    .map(|(n, kind)| {
                        div()
                            .when(n > 0, |d| d.ml(px(-4.0)))
                            .size(px(20.0))
                            .rounded_full()
                            .fill(Ink::Card, a)
                            .border(px(1.0))
                            .border_color(hsla(Ink::Hairline.color(a)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(kind, 9.0, Ink::TextMuted, a))
                    }),
            );
    let key = id.to_owned();
    div()
        .id(SharedString::from(format!("chip-{id}")))
        .h(px(28.0))
        .pl(px(4.0))
        .pr(px(10.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .rounded_full()
        .bg(hsla(Ink::Raised.color(a).with_alpha(0.7)))
        .cursor_pointer()
        .hover(|s| s.bg(hsla(Ink::Raised.color(a))))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx)))
        .child(icons)
        .child(label(
            &steps::summary(steps),
            TypeRole::Label,
            Ink::TextMuted,
            a,
        ))
        .children(steps::failed_text(steps).map(|t| label(&t, TypeRole::Label, Ink::Expired, a)))
        .child(chevron(open, 9.0, a))
}

pub(super) fn step_group(
    input: &ConversationInput,
    s: &ConversationState,
    sel: &Sel,
    steps: &[ToolRow],
    live: bool,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> Div {
    let id = sel.id();
    let open = live || s.is_open(id);
    div()
        .flex()
        .flex_col()
        .items_start()
        .child(step_chip(id, steps, open, a, cx))
        .when(open, |d| {
            d.child(
                div()
                    .w_full()
                    .mt(px(8.0))
                    .pl(px(12.0))
                    .border_l(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .children(
                        steps
                            .iter()
                            .map(|step| step_row(input, s, sel, step, live, a, cx)),
                    ),
            )
        })
        .part_at("Tool Steps", Corner::TopTrailing, a, cx)
}

pub(super) fn thinking(
    s: &ConversationState,
    id: &str,
    title: &str,
    text: &str,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> Div {
    let has_text = !text.is_empty();
    let open = has_text && s.is_open(id);
    let key = id.to_owned();
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(6.0))
        .child(
            div()
                .id(SharedString::from(format!("think-{id}")))
                .flex()
                .items_center()
                .gap(px(6.0))
                .when(has_text, |d| {
                    d.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx)))
                })
                .child(icon(Icon::Thinking, 10.0, Ink::TextFaint, a))
                .child(label(title, TypeRole::Label, Ink::TextFaint, a))
                .when(has_text, |d| d.child(chevron(open, 8.0, a)))
                .part_at("Thinking Row", Corner::Trailing, a, cx),
        )
        .when(open, |d| {
            d.child(
                div()
                    .w_full()
                    .flex()
                    .items_start()
                    .gap(px(10.0))
                    .child(div().flex_shrink_0().w(px(2.0)).fill(Ink::Hairline, a))
                    .child(
                        label(text, TypeRole::Meta, Ink::TextMuted, a)
                            .flex_1()
                            .min_w(px(0.0)),
                    ),
            )
        })
}
