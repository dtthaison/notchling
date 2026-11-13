use gpui::{div, prelude::*, px, Div, SharedString, Stateful, WeakEntity};
use notchling_kit::metric::RADIUS_SM;
use notchling_kit::ui::{hsla, label, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::inspector::{empty, fact_row, MARK_BOX, ROW_LINE};
use super::ChatView;
use crate::chat::git::{GitFile, GitModel, SHOWN_FILES};

pub fn content(m: &GitModel, chat: &WeakEntity<ChatView>, a: Appearance) -> Div {
    let mut column = div().flex().flex_col().gap(px(6.0));
    column = column.child(fact_row(Icon::Branch, &m.branch, a));
    if let Some(tracking) = &m.tracking {
        column = column.child(
            label(tracking, TypeRole::Meta, Ink::TextFaint, a)
                .pl(px(MARK_BOX + 8.0))
                .line_height(px(ROW_LINE))
                .w_full()
                .elide(),
        );
    }
    if m.files.is_empty() {
        return column.child(empty("No changes", a));
    }
    for (index, file) in m.files.iter().take(SHOWN_FILES).enumerate() {
        column = column.child(file_row(index, file, chat, a));
    }
    match m.hidden_text() {
        Some(text) => column.child(label(&text, TypeRole::Meta, Ink::TextFaint, a)),
        None => column,
    }
}

fn counts(file: &GitFile, a: Appearance) -> Div {
    let mut row = div().flex_shrink_0().flex().items_center().gap(px(4.0));
    if let Some(added) = file.added.filter(|n| *n > 0) {
        row = row.child(
            label(&format!("+{added}"), TypeRole::Meta, Ink::Done, a).line_height(px(ROW_LINE)),
        );
    }
    if let Some(removed) = file.removed.filter(|n| *n > 0) {
        row = row.child(
            label(
                &format!("\u{2212}{removed}"),
                TypeRole::Meta,
                Ink::Expired,
                a,
            )
            .line_height(px(ROW_LINE)),
        );
    }
    row
}

fn file_row(
    index: usize,
    file: &GitFile,
    chat: &WeakEntity<ChatView>,
    a: Appearance,
) -> Stateful<Div> {
    let (chat, path) = (chat.clone(), file.path.clone());
    let mut text = div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            label(file.name(), TypeRole::Meta, Ink::Text, a)
                .line_height(px(ROW_LINE))
                .w_full()
                .elide(),
        );
    if !file.folder().is_empty() {
        text = text.child(
            label(file.folder(), TypeRole::Meta, Ink::TextFaint, a)
                .line_height(px(ROW_LINE))
                .w_full()
                .elide(),
        );
    }
    div()
        .id(SharedString::from(format!("git-file-{index}")))
        .mx(px(-4.0))
        .px(px(4.0))
        .py(px(2.0))
        .rounded(px(RADIUS_SM))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(Ink::Raised.color(a).with_alpha(0.6))))
        .on_click(move |_, _, cx| {
            chat.update(cx, |this, cx| this.open_git_diff(&path, cx))
                .ok();
        })
        .flex()
        .items_start()
        .gap(px(7.0))
        .child(
            div()
                .w(px(MARK_BOX))
                .h(px(ROW_LINE))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(label(&file.status, TypeRole::Mono, file.ink(), a)),
        )
        .child(text)
        .child(counts(file, a))
}
