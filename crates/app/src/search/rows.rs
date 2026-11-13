use gpui::{div, prelude::*, px, Div, FontWeight, HighlightStyle, SharedString, StyledText};
use notchling_engine::search::{age_text, SessionTool};
use notchling_kit::metric::{RADIUS_MD, RADIUS_SM};
use notchling_kit::ui::{hsla, icon, label, tool_icon, KitStyled, ToolMark};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::marks;
use super::model::{layout, Section};
use super::wire::{Page, SessionRow};
use crate::chat::format::short_path;

pub(super) fn hairline(a: Appearance) -> Div {
    div()
        .h(px(1.0))
        .w_full()
        .flex_shrink_0()
        .fill(Ink::Hairline, a)
}

fn icon_box(selected: bool, a: Appearance) -> Div {
    div()
        .size(px(30.0))
        .flex_shrink_0()
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .fill(
            if selected {
                Ink::AccentWash
            } else {
                Ink::Raised
            },
            a,
        )
}

fn tool_mark(tool: SessionTool, selected: bool, a: Appearance) -> Div {
    match tool {
        SessionTool::Claude => icon_box(selected, a).child(tool_icon(ToolMark::Claude, 20.0, a)),
        SessionTool::Codex => icon_box(selected, a).child(tool_icon(ToolMark::Codex, 20.0, a)),
        SessionTool::Antigravity => {
            icon_box(selected, a).child(tool_icon(ToolMark::Antigravity, 20.0, a))
        }
    }
}

pub(super) fn section_header(section: Section, a: Appearance) -> Div {
    div()
        .h(px(layout::HEADER))
        .flex_shrink_0()
        .px(px(10.0))
        .pb(px(4.0))
        .flex()
        .items_end()
        .child(label(section.title(), TypeRole::Section, Ink::TextFaint, a))
}

fn row_frame(height: f32, selected: bool, a: Appearance) -> Div {
    div()
        .h(px(height))
        .w_full()
        .flex_shrink_0()
        .px(px(10.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .rounded(px(RADIUS_MD))
        .when(selected, |r| r.fill(Ink::Selection, a))
}

pub(super) fn page_row(page: &Page, selected: bool, now_secs: f64, a: Appearance) -> Div {
    let subtitle: Vec<&str> = [page.project.as_str(), page.account.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    row_frame(layout::PAGE_ROW, selected, a)
        .child(icon_box(selected, a).child(icon(
            Icon::Page,
            TypeRole::Heading.spec().size,
            if selected {
                Ink::AccentText
            } else {
                Ink::TextMuted
            },
            a,
        )))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(label(&page.title, TypeRole::Heading, Ink::Text, a).elide())
                .child(label(&subtitle.join(" · "), TypeRole::Meta, Ink::TextMuted, a).elide()),
        )
        .child(label(
            &age_text(page.updated_secs, now_secs),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ))
}

pub(super) fn secret_row(
    entry: &notchling_engine::vault::VaultEntry,
    selected: bool,
    a: Appearance,
) -> Div {
    let note = entry.note.lines().next().unwrap_or("").trim();
    let detail: Vec<&str> = [entry.url.as_deref().unwrap_or(""), note]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    row_frame(layout::PAGE_ROW, selected, a)
        .child(icon_box(selected, a).child(icon(
            Icon::Key,
            TypeRole::Heading.spec().size,
            if selected {
                Ink::AccentText
            } else {
                Ink::TextMuted
            },
            a,
        )))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(label(&entry.name, TypeRole::Heading, Ink::Text, a).elide())
                .child(label(&detail.join(" · "), TypeRole::Meta, Ink::TextMuted, a).elide()),
        )
}

fn session_detail(session: &SessionRow, home: &std::path::Path, a: Appearance) -> Div {
    let line = div()
        .kit_type(TypeRole::Meta)
        .ink(Ink::TextMuted, a)
        .elide();
    match &session.snippet {
        Some(snippet) => {
            let bold = HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                color: Some(hsla(Ink::Text.color(a))),
                ..Default::default()
            };
            let ranges = marks::clean(snippet, &session.highlights)
                .into_iter()
                .map(|range| (range, bold));
            line.child(StyledText::new(SharedString::from(snippet.clone())).with_highlights(ranges))
        }
        None => line.child(short_path(&session.folder, home)),
    }
}

pub(super) fn session_row(
    session: &SessionRow,
    selected: bool,
    now_secs: f64,
    home: &std::path::Path,
    a: Appearance,
) -> Div {
    let place = std::path::Path::new(&session.folder)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_owned();
    row_frame(layout::SESSION_ROW, selected, a)
        .child(tool_mark(session.tool, selected, a))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            label(&session.title, TypeRole::Heading, Ink::Text, a)
                                .flex_1()
                                .min_w(px(0.0))
                                .elide(),
                        )
                        .child(label(&place, TypeRole::Meta, Ink::TextMuted, a).elide())
                        .child(label(
                            &age_text(session.updated_secs, now_secs),
                            TypeRole::Meta,
                            Ink::TextFaint,
                            a,
                        )),
                )
                .child(session_detail(session, home, a)),
        )
}
