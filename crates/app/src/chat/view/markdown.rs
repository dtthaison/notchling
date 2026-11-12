//! A reply's Markdown drawn with the kit (`MarkdownView` in the Swift app): `chat::markdown`'s
//! blocks, each paragraph's spans as one styled text, links clickable (`page_panel::follow_link`:
//! an Artifact opens beside the chat, anything else in the browser).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{
    div, font, img, prelude::*, px, AnyElement, ClipboardItem, Div, FontStyle, FontWeight,
    InteractiveText, MouseButton, ObjectFit, SharedString, StrikethroughStyle, StyledText, TextRun,
    UnderlineStyle,
};
use notchling_kit::metric::RADIUS_MD;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button_with, hsla, icon, label, KitStyled, MONO_FONT, SYSTEM_FONT};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use crate::chat::markdown::{blocks, image_links, spans, Block, LinkPress, ListItem, Span};
use crate::chat::paths::{self, Probe};

/// Body lines get a little air, as the Swift app's `lineSpacing(3)`.
const BODY_LINE: f32 = 20.0;
/// A linked image's width under its paragraph; its height follows its shape.
const IMAGE_WIDTH: f32 = 360.0;

/// The spans of `text` as one text element: bold, italics, code (the mono face on a raised wash),
/// strikethrough, links (accent, underlined, clickable). `weight` is the text's own weight (a
/// heading's), `ink` its color. `key` makes the element's id unique in the window.
fn inline(
    key: &str,
    text: &str,
    weight: FontWeight,
    ink: Ink,
    bases: &[PathBuf],
    a: Appearance,
) -> AnyElement {
    let spans = link_paths(spans(text), bases);
    let mut full = String::new();
    let mut runs: Vec<TextRun> = Vec::with_capacity(spans.len());
    let mut links: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for span in &spans {
        let start = full.len();
        full.push_str(&span.text);
        runs.push(run(span, weight, ink, a));
        if let Some(url) = &span.link {
            links.push((start..full.len(), url.clone()));
        }
    }
    let plain_text = full.clone();
    let styled = StyledText::new(SharedString::from(full)).with_runs(runs);
    let text_layout = styled.layout().clone();
    let select_key = format!("{key}-text");
    if links.is_empty() {
        return super::select::selectable(
            select_key,
            plain_text,
            text_layout,
            styled.into_any_element(),
            a,
        )
        .into_any_element();
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
    // The pointing hand over links comes from the clickable text; the click itself is ours, on
    // listeners that are there every frame (`LinkPress`).
    let text = InteractiveText::new(id.clone(), styled).on_click(hand, |_, _, _| {});
    let linked = div()
        .id(id)
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
    super::select::selectable(
        select_key,
        plain_text,
        text_layout,
        linked.into_any_element(),
        a,
    )
    .into_any_element()
}

/// An image file's width and height, read once from the first bytes of the file.
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

/// How long what the disk said about a path is kept: a message is drawn every frame, and a path
/// in it is looked up each time.
const DISK_TTL: Duration = Duration::from_secs(3);

/// What the disk has, remembered for [`DISK_TTL`].
struct CachedDisk {
    home: Option<PathBuf>,
}

type Seen = std::collections::HashMap<PathBuf, (Instant, bool, bool)>;

impl CachedDisk {
    fn look(&self, path: &Path) -> (bool, bool) {
        SEEN.with(|seen| {
            let mut seen = seen.borrow_mut();
            if let Some((at, exists, dir)) = seen.get(path) {
                if at.elapsed() < DISK_TTL {
                    return (*exists, *dir);
                }
            }
            if seen.len() > 4096 {
                seen.clear();
            }
            let found = (path.exists(), path.is_dir());
            seen.insert(path.to_path_buf(), (Instant::now(), found.0, found.1));
            found
        })
    }
}

impl Probe for CachedDisk {
    fn exists(&self, path: &Path) -> bool {
        self.look(path).0
    }
    fn is_dir(&self, path: &Path) -> bool {
        self.look(path).1
    }
    fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }
}

fn disk() -> CachedDisk {
    CachedDisk {
        home: std::env::var_os("HOME").map(PathBuf::from),
    }
}

/// The folders a message's relative paths are read against.
fn path_bases(text: &str, folder: &str) -> Vec<PathBuf> {
    paths::bases(text, folder, &disk())
}

/// The spans with the paths that exist made links (`chat::paths`).
fn link_paths(spans: Vec<Span>, bases: &[PathBuf]) -> Vec<Span> {
    if bases.is_empty()
        && !spans
            .iter()
            .any(|s| s.text.contains('/') || s.link.is_some())
    {
        return spans;
    }
    paths::link_spans(spans, bases, &disk())
}

thread_local! {
    /// What the disk said about each path lately.
    static SEEN: std::cell::RefCell<Seen> = Default::default();
    /// Image shapes read so far, by path.
    static SHAPES: std::cell::RefCell<std::collections::HashMap<String, Option<(u32, u32)>>> =
        Default::default();
    /// The link press under way; one pointer, so one press at a time.
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

/// Copies `text`; the small ghost button on code boxes.
pub fn copy_button(key: &str, text: &str, a: Appearance) -> impl IntoElement {
    let text = text.to_owned();
    // The Swift app's `CopyButton`: the copy icon (10 pt) and the word, 4 apart, in the ghost
    // button's text ink.
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

/// Monospaced text in a box with a title row (`MonoBox`): "Command", "Output", or a code block's
/// language. `error` tints it red.
pub fn mono_box(key: &str, title: &str, text: &str, error: bool, a: Appearance) -> Div {
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
                .child(plain(&format!("{key}-text"), text, a)),
        )
}

/// Text in its parent's style, selectable under `key` (`select`).
pub fn plain(key: &str, text: &str, a: Appearance) -> AnyElement {
    let styled = StyledText::new(SharedString::from(text.to_owned()));
    let layout = styled.layout().clone();
    super::select::selectable(key, text, layout, styled.into_any_element(), a).into_any_element()
}

fn heading(
    key: &str,
    level: usize,
    text: &str,
    first: bool,
    bases: &[PathBuf],
    a: Appearance,
) -> Div {
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
        .child(inline(key, text, weight, Ink::Text, bases, a))
}

fn paragraph(key: &str, text: &str, ink: Ink, bases: &[PathBuf], a: Appearance) -> Div {
    let images = image_links(text)
        .into_iter()
        .filter(|path| std::path::Path::new(path).is_file());
    div()
        .kit_type(TypeRole::Body)
        .line_height(px(BODY_LINE))
        .ink(ink, a)
        .child(inline(key, text, FontWeight::NORMAL, ink, bases, a))
        .children(images.map(|path| image(key, &path, a)))
}

/// A linked image on this Mac, drawn under its paragraph (Codex's generated images); a click
/// opens it beside the chat, as its link does.
fn image(key: &str, path: &str, a: Appearance) -> impl IntoElement {
    let to = path.to_owned();
    // GPUI sizes an image's box from its pixels, not its width: the height comes from its shape.
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

fn list(key: &str, items: &[ListItem], bases: &[PathBuf], a: Appearance) -> Div {
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
                    paragraph(&format!("{key}-{i}"), &item.text, Ink::Text, bases, a)
                        .flex_1()
                        .min_w(px(0.0)),
                )
        }))
}

fn quote(key: &str, text: &str, bases: &[PathBuf], a: Appearance) -> Div {
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
            paragraph(key, text, Ink::TextMuted, bases, a)
                .flex_1()
                .min_w(px(0.0)),
        )
}

fn code(key: &str, language: &str, text: &str, a: Appearance) -> Div {
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
                .child(plain(&format!("{key}-code"), text, a)),
        )
}

fn table(
    key: &str,
    header: &[String],
    rows: &[Vec<String>],
    bases: &[PathBuf],
    a: Appearance,
) -> Div {
    let cell = |key: String, text: &str, head: bool| {
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
                text,
                if head {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                },
                Ink::Text,
                bases,
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

/// The message as blocks, 10 pt apart. `key` makes the ids unique (the item's id); `folder` is
/// the session's (or the file's own), which a path in the text is read against after the folders
/// the text names (`chat::paths`).
pub fn render(key: &str, text: &str, folder: &str, a: Appearance) -> Div {
    let blocks = blocks(text);
    let bases = path_bases(text, folder);
    let bases = bases.as_slice();
    let mut column = div().w_full().flex().flex_col().gap(px(10.0));
    for (i, block) in blocks.iter().enumerate() {
        let k = format!("{key}-{i}");
        column = column.child(match block {
            Block::Heading { level, text } => heading(&k, *level, text, i == 0, bases, a),
            Block::Paragraph(text) => paragraph(&k, text, Ink::Text, bases, a),
            Block::List(items) => list(&k, items, bases, a),
            Block::Quote(text) => quote(&k, text, bases, a),
            Block::Code { language, text } => code(&k, language, text, a),
            Block::Table { header, rows } => table(&k, header, rows, bases, a),
            Block::Rule => div().h(px(1.0)).my(px(4.0)).fill(Ink::Hairline, a),
        });
    }
    column
}
