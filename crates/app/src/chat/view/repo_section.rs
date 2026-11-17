use gpui::{
    div, prelude::*, px, Div, HighlightStyle, SharedString, Stateful, StyledText, WeakEntity,
};
use notchling_kit::metric::{PILL_HEIGHT, RADIUS_SM};
use notchling_kit::ui::{hsla, icon, label, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::inspector::{MARK_BOX, ROW_LINE};
use super::ChatView;
use crate::chat::changes_target::Target;
use crate::chat::worktrees::{CurrentRow, GitSection, OtherRow, SubPart};

const DOT: f32 = 7.0;

pub fn content(section: &GitSection, chat: &WeakEntity<ChatView>, a: Appearance) -> Div {
    let mut column = div().flex().flex_col().gap(px(6.0));
    column = column.child(current_row(&section.current, chat, a));
    for (index, row) in section.others.iter().enumerate() {
        column = column.child(other_row(index, row, chat, a));
    }
    match &section.more {
        Some(text) => column.child(more_row(text, chat, a)),
        None => column,
    }
}

fn shell(id: impl Into<gpui::ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .mx(px(-4.0))
        .px(px(4.0))
        .py(px(2.0))
        .rounded(px(RADIUS_SM))
}

fn clickable(
    row: Stateful<Div>,
    chat: &WeakEntity<ChatView>,
    target: Target,
    a: Appearance,
) -> Stateful<Div> {
    let chat = chat.clone();
    row.cursor_pointer()
        .hover(|style| style.bg(hsla(Ink::Raised.color(a).with_alpha(0.6))))
        .on_click(move |_, _, cx| {
            let target = target.clone();
            chat.update(cx, |this, cx| this.open_changes_for(target, false, cx))
                .ok();
        })
}

fn icon_column(shape: Icon, ink: Ink, a: Appearance) -> Div {
    div()
        .w(px(MARK_BOX))
        .h(px(ROW_LINE))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(icon(shape, 10.0, ink, a))
}

fn counts(right: Option<(u64, u64)>, a: Appearance) -> Div {
    let mut row = div().flex_shrink_0().flex().items_center().gap(px(4.0));
    if let Some((added, removed)) = right {
        row = row
            .child(
                label(&format!("+{added}"), TypeRole::Meta, Ink::Done, a).line_height(px(ROW_LINE)),
            )
            .child(
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

pub(super) fn sub_line(parts: &[SubPart], a: Appearance) -> Div {
    let mut text = String::new();
    let mut marks: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    for part in parts {
        if !text.is_empty() {
            text.push(' ');
        }
        let start = text.len();
        let ink = match part {
            SubPart::Faint(shown) | SubPart::Running(shown) => {
                text.push_str(shown);
                matches!(part, SubPart::Running(_)).then_some(Ink::AccentText)
            }
            SubPart::Added(n) => {
                text.push_str(&format!("+{n}"));
                Some(Ink::Done)
            }
            SubPart::Removed(n) => {
                text.push_str(&format!("\u{2212}{n}"));
                Some(Ink::Expired)
            }
        };
        if let Some(ink) = ink {
            let style = HighlightStyle {
                color: Some(hsla(ink.color(a))),
                ..Default::default()
            };
            marks.push((start..text.len(), style));
        }
    }
    div()
        .w_full()
        .kit_type(TypeRole::Meta)
        .ink(Ink::TextFaint, a)
        .line_height(px(ROW_LINE))
        .child(StyledText::new(SharedString::from(text)).with_highlights(marks))
}

fn name_line(branch: &str, ink: Ink, extra: Option<Div>, a: Appearance) -> Div {
    let mut line = div().w_full().flex().items_center().gap(px(6.0));
    if let Some(extra) = extra {
        line = line.child(extra);
    }
    line.child(
        label(branch, TypeRole::Meta, ink, a)
            .line_height(px(ROW_LINE))
            .min_w(px(0.0))
            .elide(),
    )
}

pub(super) fn pill(text: &str, a: Appearance) -> Div {
    div()
        .h(px(PILL_HEIGHT - 4.0))
        .px(px(7.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .rounded_full()
        .bg(hsla(Ink::Raised.color(a)))
        .child(label(text, TypeRole::Meta, Ink::TextFaint, a))
}

pub(super) fn working_dot(a: Appearance) -> Div {
    div()
        .w(px(DOT))
        .h(px(DOT))
        .flex_shrink_0()
        .rounded_full()
        .bg(hsla(Ink::Working.color(a)))
}

fn body(name: Div, sub: &[SubPart], right: Option<(u64, u64)>, a: Appearance) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .w_full()
                .flex()
                .items_start()
                .gap(px(7.0))
                .child(icon_column(Icon::Branch, Ink::TextFaint, a))
                .child(div().flex_1().min_w(px(0.0)).child(name))
                .child(counts(right, a)),
        )
        .child(div().pl(px(MARK_BOX + 7.0)).child(sub_line(sub, a)))
}

fn current_row(row: &CurrentRow, chat: &WeakEntity<ChatView>, a: Appearance) -> Stateful<Div> {
    let name = div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            label(&row.branch, TypeRole::Meta, Ink::Text, a)
                .line_height(px(ROW_LINE))
                .min_w(px(0.0))
                .elide(),
        )
        .child(pill(notchling_locale::chat::git_this_session(), a));
    let item = shell("git-current").child(body(name, &row.sub, row.right, a));
    clickable(item, chat, Target::Own, a)
}

fn other_row(
    index: usize,
    row: &OtherRow,
    chat: &WeakEntity<ChatView>,
    a: Appearance,
) -> Stateful<Div> {
    let ink = if row.missing {
        Ink::TextFaint
    } else {
        Ink::Text
    };
    let dot = row.working.then(|| working_dot(a));
    let name = name_line(&row.branch, ink, dot, a);
    let item = shell(SharedString::from(format!("git-worktree-{index}")))
        .child(body(name, &row.sub, row.right, a));
    if row.missing {
        return item;
    }
    clickable(item, chat, Target::Worktree(row.path.clone()), a)
}

fn more_row(text: &str, chat: &WeakEntity<ChatView>, a: Appearance) -> Stateful<Div> {
    let chat = chat.clone();
    shell("git-more")
        .flex()
        .items_center()
        .gap(px(4.0))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(Ink::Raised.color(a).with_alpha(0.6))))
        .on_click(move |_, _, cx| {
            chat.update(cx, |this, cx| this.open_branches(cx)).ok();
        })
        .child(
            label(text, TypeRole::Meta, Ink::TextMuted, a)
                .line_height(px(ROW_LINE))
                .min_w(px(0.0))
                .elide(),
        )
        .child(icon(Icon::ChevronRight, 9.0, Ink::TextFaint, a).flex_shrink_0())
}
