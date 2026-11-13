use std::cell::Cell;

use gpui::{
    div, font, img, prelude::*, px, AnyElement, ClipboardItem, Div, FontStyle, FontWeight,
    InteractiveText, MouseButton, ObjectFit, SharedString, StrikethroughStyle, StyledText,
    TextLayout, TextRun, UnderlineStyle,
};
use notchling_kit::metric::RADIUS_MD;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button_with, hsla, icon, label, KitStyled, MONO_FONT, SYSTEM_FONT};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::row_select::{self, SelState};
use crate::chat::markdown::{LinkPress, Span};
use crate::chat::markdown_cache::{spans_text, Inline, Item, Part};

const BODY_LINE: f32 = 20.0;
const IMAGE_WIDTH: f32 = 360.0;

pub enum Sel<'a> {
    Row {
        row: &'a SharedString,
        next: &'a Cell<u16>,
        state: &'a SelState,
    },
    File,
}

impl Sel<'_> {
    pub fn id(&self) -> &str {
        match self {
            Sel::Row { row, .. } => row,
            Sel::File => "",
        }
    }
}

fn selectable(
    sel: &Sel,
    key: String,
    text: String,
    layout: TextLayout,
    child: AnyElement,
    a: Appearance,
) -> AnyElement {
    match sel {
        Sel::Row { row, next, state } => {
            let block = next.get();
            next.set(block.saturating_add(1));
            row_select::selectable((*row).clone(), block, text, layout, child, a, state)
                .into_any_element()
        }
        Sel::File => super::select::selectable(key, text, layout, child, a).into_any_element(),
    }
}

fn inline(
    key: &str,
    sel: &Sel,
    spans: &[Span],
    weight: FontWeight,
    ink: Ink,
    a: Appearance,
) -> AnyElement {
    let full = spans_text(spans);
    let mut runs: Vec<TextRun> = Vec::with_capacity(spans.len());
    let mut links: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut start = 0;
    for span in spans {
        let end = start + span.text.len();
        runs.push(run(span, weight, ink, a));
        if let Some(url) = &span.link {
            links.push((start..end, url.clone()));
        }
        start = end;
    }
    let plain_text = full.clone();
    let styled = StyledText::new(SharedString::from(full)).with_runs(runs);
    let text_layout = styled.layout().clone();
    let select_key = format!("{key}-text");
    if links.is_empty() {
        return selectable(
            sel,
            select_key,
            plain_text,
            text_layout,
            styled.into_any_element(),
            a,
        );
    }
    let (ranges, urls): (Vec<_>, Vec<_>) = links.into_iter().unzip();
    let layout = styled.layout().clone();
    let hand = ranges.clone();
    let link_at = move |position| {
        let index = layout.index_for_position(position).ok()?;
        ranges.iter().position(|r| r.contains(&index))
    };
    let pressed = link_at.clone();
    let (down_key, up_key) = (key.to_owned(), key.to_owned());
    let id = SharedString::from(format!("md-{key}"));
    let text = InteractiveText::new(id, styled).on_click(hand, |_, _, _| {});
    let linked = div()
        .on_mouse_down(MouseButton::Left, move |event, _, _| {
            PRESS.with(|p| p.borrow_mut().down(&down_key, pressed(event.position)));
        })
        .on_mouse_up(MouseButton::Left, move |event, window, cx| {
            let open = PRESS.with(|p| p.borrow_mut().up(&up_key, link_at(event.position)));
            if let Some(url) = open.and_then(|i| urls.get(i)) {
                super::page_panel::follow_link(url, window, cx);
            }
        })
        .child(text);
    selectable(
        sel,
        select_key,
        plain_text,
        text_layout,
        linked.into_any_element(),
        a,
    )
}

pub(super) fn image_shape(path: &str) -> Option<(u32, u32)> {
    if let Some(known) = SHAPES.with(|s| s.borrow().get(path).copied()) {
        return known;
    }
    let shape = (|| {
        use std::io::Read;
        let mut head = Vec::with_capacity(64 * 1024);
        std::fs::File::open(path)
            .ok()?
            .take(64 * 1024)
            .read_to_end(&mut head)
            .ok()?;
        crate::chat::image_size::size(&head)
    })();
    SHAPES.with(|s| s.borrow_mut().insert(path.to_owned(), shape));
    shape
}

thread_local! {
    static SHAPES: std::cell::RefCell<std::collections::HashMap<String, Option<(u32, u32)>>> =
        Default::default();
    static PRESS: std::cell::RefCell<LinkPress> = std::cell::RefCell::new(LinkPress::default());
}

fn run(span: &Span, weight: FontWeight, ink: Ink, a: Appearance) -> TextRun {
    let mut font = font(if span.code { MONO_FONT } else { SYSTEM_FONT });
    font.weight = if span.bold { FontWeight::BOLD } else { weight };
    font.style = if span.italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    };
    let linked = span.link.is_some();
    let color = if linked {
        hsla(Ink::AccentText.color(a))
    } else {
        hsla(ink.color(a))
    };
    TextRun {
        len: span.text.len(),
        font,
        color,
        background_color: span.code.then(|| hsla(Ink::Raised.color(a))),
        underline: linked.then(|| UnderlineStyle {
            thickness: px(1.0),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: span.strike.then(|| StrikethroughStyle {
            thickness: px(1.0),
            color: Some(color),
        }),
    }
}

pub fn copy_button(key: &str, text: &str, a: Appearance) -> impl IntoElement {
    let text = text.to_owned();
    let content = div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(icon(Icon::Copy, 10.0, Ink::Text, a))
        .child("Copy");
    button_with(
        SharedString::from(format!("copy-{key}")),
        content,
        ButtonKind::Ghost,
        PartState::Rest,
        a,
    )
    .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone())))
}

pub fn mono_box(key: &str, sel: &Sel, title: &str, text: &str, error: bool, a: Appearance) -> Div {
    let title_ink = if error { Ink::Expired } else { Ink::TextMuted };
    let (fill, edge) = if error {
        let red = Ink::Expired.color(a);
        (red.with_alpha(0.08), red.with_alpha(0.3))
    } else {
        (Ink::Raised.color(a).with_alpha(0.7), Ink::Hairline.color(a))
    };
    div()
        .w_full()
        .rounded(px(RADIUS_MD))
        .bg(hsla(fill))
        .border(px(1.0))
        .border_color(hsla(edge))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(26.0))
                .pl(px(10.0))
                .pr(px(4.0))
                .flex()
                .items_center()
                .child(label(title, TypeRole::Meta, title_ink, a))
                .child(div().flex_1())
                .child(copy_button(key, text, a)),
        )
        .child(
            div()
                .px(px(10.0))
                .pb(px(10.0))
                .kit_type(TypeRole::Mono)
                .ink(Ink::Text, a)
                .child(plain(&format!("{key}-text"), sel, text, a)),
        )
}

pub fn plain(key: &str, sel: &Sel, text: &str, a: Appearance) -> AnyElement {
    let styled = StyledText::new(SharedString::from(text.to_owned()));
    let layout = styled.layout().clone();
    selectable(
        sel,
        key.to_owned(),
        text.to_owned(),
        layout,
        styled.into_any_element(),
        a,
    )
}

fn heading(key: &str, sel: &Sel, level: usize, spans: &[Span], first: bool, a: Appearance) -> Div {
    let size = match level {
        1 => 18.0,
        2 => 16.0,
        3 => 14.0,
        _ => 13.0,
    };
    let weight = if level <= 2 {
        FontWeight::BOLD
    } else {
        FontWeight::SEMIBOLD
    };
    let top = if first {
        0.0
    } else if level <= 2 {
        4.0
    } else {
        2.0
    };
    div()
        .pt(px(top))
        .font_family(SYSTEM_FONT)
        .text_size(px(size))
        .font_weight(weight)
        .ink(Ink::Text, a)
        .child(inline(key, sel, spans, weight, Ink::Text, a))
}

fn paragraph(key: &str, sel: &Sel, text: &Inline, ink: Ink, a: Appearance) -> Div {
    div()
        .kit_type(TypeRole::Body)
        .line_height(px(BODY_LINE))
        .ink(ink, a)
        .child(inline(key, sel, &text.spans, FontWeight::NORMAL, ink, a))
        .children(text.images.iter().map(|path| image(key, path, a)))
}

fn image(key: &str, path: &str, a: Appearance) -> impl IntoElement {
    let to = path.to_owned();
    let (w, h) = image_shape(path).unwrap_or((4, 3));
    div()
        .id(SharedString::from(format!("md-img-{key}-{path}")))
        .mt(px(8.0))
        .w(px(IMAGE_WIDTH))
        .h(px(IMAGE_WIDTH * h as f32 / w as f32))
        .rounded(px(RADIUS_MD))
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .overflow_hidden()
        .cursor_pointer()
        .on_click(move |_, window, cx| super::page_panel::follow_link(&to, window, cx))
        .child(
            img(std::path::PathBuf::from(path))
                .size_full()
                .rounded(px(RADIUS_MD))
                .object_fit(ObjectFit::Contain),
        )
}

fn list(key: &str, sel: &Sel, items: &[Item], a: Appearance) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .children(items.iter().enumerate().map(|(i, item)| {
            let bullet = item.marker == "•";
            let marker = div()
                .flex_shrink_0()
                .min_w(px(14.0))
                .flex()
                .justify_end()
                .font_family(SYSTEM_FONT)
                .text_size(px(13.0))
                .line_height(px(BODY_LINE))
                .font_weight(if bullet {
                    FontWeight::BOLD
                } else {
                    FontWeight::SEMIBOLD
                })
                .ink(if bullet { Ink::Accent } else { Ink::TextMuted }, a)
                .child(item.marker.clone());
            div()
                .pl(px(item.depth as f32 * 18.0))
                .flex()
                .items_start()
                .gap(px(8.0))
                .child(marker)
                .child(
                    paragraph(&format!("{key}-{i}"), sel, &item.inline, Ink::Text, a)
                        .flex_1()
                        .min_w(px(0.0)),
                )
        }))
}

fn quote(key: &str, sel: &Sel, text: &Inline, a: Appearance) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(10.0))
        .child(
            div()
                .flex_shrink_0()
                .w(px(3.0))
                .rounded(px(2.0))
                .bg(hsla(Ink::Accent.color(a).with_alpha(0.6))),
        )
        .child(
            paragraph(key, sel, text, Ink::TextMuted, a)
                .flex_1()
                .min_w(px(0.0)),
        )
}

fn code(key: &str, sel: &Sel, language: &str, text: &str, a: Appearance) -> Div {
    let title = if language.is_empty() {
        "code"
    } else {
        language
    };
    div()
        .w_full()
        .rounded(px(RADIUS_MD))
        .bg(hsla(Ink::Raised.color(a).with_alpha(0.7)))
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(28.0))
                .pl(px(12.0))
                .pr(px(6.0))
                .flex()
                .items_center()
                .border_b(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
                .child(label(title, TypeRole::Meta, Ink::TextMuted, a))
                .child(div().flex_1())
                .child(copy_button(key, text, a)),
        )
        .child(
            div()
                .p(px(12.0))
                .kit_type(TypeRole::Mono)
                .ink(Ink::Text, a)
                .child(plain(&format!("{key}-code"), sel, text, a)),
        )
}

fn table(
    key: &str,
    sel: &Sel,
    header: &[Vec<Span>],
    rows: &[Vec<Vec<Span>>],
    a: Appearance,
) -> Div {
    let cell = |key: String, text: &[Span], head: bool| {
        div()
            .flex_1()
            .min_w(px(60.0))
            .px(px(12.0))
            .py(px(8.0))
            .font_family(SYSTEM_FONT)
            .text_size(px(12.5))
            .line_height(px(18.0))
            .font_weight(if head {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            })
            .ink(Ink::Text, a)
            .child(inline(
                &key,
                sel,
                text,
                if head {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                },
                Ink::Text,
                a,
            ))
    };
    let mut table = div()
        .w_full()
        .rounded(px(RADIUS_MD))
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .overflow_hidden()
        .fill(Ink::Card, a)
        .flex()
        .flex_col()
        .child(
            div().flex().fill(Ink::Raised, a).children(
                header
                    .iter()
                    .enumerate()
                    .map(|(c, text)| cell(format!("{key}-h{c}"), text, true)),
            ),
        );
    for (r, row) in rows.iter().enumerate() {
        let shaded = r % 2 == 1;
        table = table.child(div().h(px(1.0)).fill(Ink::Hairline, a)).child(
            div()
                .flex()
                .when(shaded, |d| {
                    d.bg(hsla(Ink::Raised.color(a).with_alpha(0.35)))
                })
                .children(
                    row.iter()
                        .enumerate()
                        .map(|(c, text)| cell(format!("{key}-{r}-{c}"), text, false)),
                ),
        );
    }
    table
}

pub fn render(key: &str, sel: &Sel, message: &[Part], a: Appearance) -> Div {
    let mut column = div().w_full().flex().flex_col().gap(px(10.0));
    for (i, part) in message.iter().enumerate() {
        let k = format!("{key}-{i}");
        column = column.child(match part {
            Part::Heading { level, spans } => heading(&k, sel, *level, spans, i == 0, a),
            Part::Paragraph(text) => paragraph(&k, sel, text, Ink::Text, a),
            Part::List(items) => list(&k, sel, items, a),
            Part::Quote(text) => quote(&k, sel, text, a),
            Part::Code { language, text } => code(&k, sel, language, text, a),
            Part::Table { header, rows } => table(&k, sel, header, rows, a),
            Part::Rule => div().h(px(1.0)).my(px(4.0)).fill(Ink::Hairline, a),
        });
    }
    column
}
