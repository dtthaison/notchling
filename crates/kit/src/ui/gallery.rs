use gpui::{div, prelude::*, px, Context, Div, Entity, Render, SharedString, Subscription, Window};

use super::icons::{icon, spinner};
use super::scrollbar::scrollable;
use super::text_editor::TextEditor;
use super::widgets::{
    account_line, avatar, button, claude_mark, field, label, option_row, session_row,
    session_row_marked, status_pill, tool_icon, ToolMark,
};
use super::{hsla, window_appearance, KitStyled};
use crate::metric::{RADIUS_MD, SPINNER_MINI, SPINNER_SMALL, TOOL_ICON};
use crate::part::{ButtonKind, PartState};
use crate::{Appearance, Icon, Ink, Status, TypeRole};

pub struct KitGallery {
    appearance: Option<Appearance>,
    editors: Vec<Entity<TextEditor>>,
    _appearance_changes: Subscription,
}

impl KitGallery {
    pub fn new(
        appearance: Option<Appearance>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let this = cx.weak_entity();
        let changes = window.observe_window_appearance(move |_, cx| {
            this.update(cx, |_, cx| cx.notify()).ok();
        });
        Self {
            appearance,
            editors: editors(cx),
            _appearance_changes: changes,
        }
    }

    pub fn appearance(&self, window: &Window) -> Appearance {
        self.appearance.unwrap_or_else(|| window_appearance(window))
    }
}

fn editors(cx: &mut Context<KitGallery>) -> Vec<Entity<TextEditor>> {
    vec![
        cx.new(|cx| TextEditor::new("Reply…", 8, cx).preview()),
        cx.new(|cx| {
            let mut e = TextEditor::new("", 8, cx).preview();
            e.set_text(
                "A reply long enough to wrap onto a second row of the box, with the caret here",
                cx,
            );
            e.select(46, 46, cx);
            e
        }),
        cx.new(|cx| {
            let mut e = TextEditor::new("", 8, cx).preview();
            e.set_text("Drag over a few words to select them", cx);
            e.select(16, 26, cx);
            e
        }),
        cx.new(|cx| {
            let mut e = TextEditor::new("", 8, cx).preview();
            e.set_text("xin chào việt", cx);
            e.mark(10..16, cx);
            e
        }),
        cx.new(|cx| {
            let mut e = TextEditor::new("", 8, cx).preview();
            e.set_text("Off while acting is off", cx);
            e.set_enabled(false, cx);
            e
        }),
    ]
}

fn text_editors(editors: &[Entity<TextEditor>], a: Appearance) -> Div {
    div()
        .w(px(320.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .children(editors.iter().map(|editor| {
            div()
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(RADIUS_MD))
                .fill(Ink::Card, a)
                .border(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
                .kit_type(TypeRole::Body)
                .line_height(px(16.0))
                .ink(Ink::Text, a)
                .child(editor.clone())
        }))
}

fn scroll_demo(a: Appearance) -> Div {
    div()
        .w(px(260.0))
        .h(px(150.0))
        .rounded(px(RADIUS_MD))
        .fill(Ink::Sidebar, a)
        .overflow_hidden()
        .child(scrollable(
            div()
                .id("gallery-scroll")
                .size_full()
                .overflow_y_scroll()
                .p(px(8.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .children((1..=14).map(|n| {
                    label(
                        &format!("A row of the list, {n} of 14"),
                        TypeRole::Body,
                        Ink::Text,
                        a,
                    )
                })),
        ))
}

fn icon_grid(a: Appearance) -> Div {
    div()
        .w(px(440.0))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .children(Icon::ALL.map(|shape| {
            div()
                .w(px(34.0))
                .h(px(28.0))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(shape, 13.0, Ink::Text, a))
        }))
        .child(spinner("gallery-mini", SPINNER_MINI, Ink::Working, a))
        .child(spinner("gallery-small", SPINNER_SMALL, Ink::Working, a))
}

fn id(part: &str, detail: &str) -> SharedString {
    SharedString::from(format!("{part}-{detail}"))
}

fn section(title: &str, appearance: Appearance, content: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(8.0))
        .child(label(title, TypeRole::Section, Ink::TextFaint, appearance))
        .child(content)
}

fn buttons(a: Appearance) -> Div {
    let states = [
        PartState::Rest,
        PartState::Hover,
        PartState::Pressed,
        PartState::Focused,
        PartState::Disabled,
    ];
    div()
        .flex()
        .items_start()
        .gap(px(16.0))
        .children(ButtonKind::ALL.map(|kind| {
            div().flex().flex_col().items_start().gap(px(6.0)).children(
                states.map(|state| {
                    button(id(kind.name(), state.name()), state.name(), kind, state, a)
                }),
            )
        }))
}

fn rows(a: Appearance) -> Div {
    let states = [
        PartState::Rest,
        PartState::Hover,
        PartState::Pressed,
        PartState::Selected,
    ];
    div()
        .w(px(260.0))
        .p(px(8.0))
        .rounded(px(RADIUS_MD))
        .fill(Ink::Sidebar, a)
        .flex()
        .flex_col()
        .gap(px(4.0))
        .children(states.map(|state| {
            session_row(
                id("row", state.name()),
                "design system",
                &state.name().to_lowercase(),
                "2m",
                Status::Needs,
                state,
                false,
                a,
            )
        }))
        .child(session_row(
            id("row", "selected-focused"),
            "design system",
            "selected + focused",
            "2m",
            Status::Needs,
            PartState::Selected,
            true,
            a,
        ))
        .children(
            [
                (ToolMark::Claude, Some(("me@example.com", false))),
                (ToolMark::Codex, Some(("ops@example.com", true))),
                (ToolMark::Antigravity, None),
            ]
            .map(|(tool, account)| {
                session_row_marked(
                    id("marked-row", tool.name()),
                    "design system",
                    tool_icon(tool, TOOL_ICON, a),
                    account.map(|(text, lane)| account_line(text, lane, a)),
                    "Asking you a question",
                    "2m",
                    Status::Needs,
                    PartState::Rest,
                    false,
                    a,
                )
            }),
        )
}

fn options(a: Appearance) -> Div {
    let states = [PartState::Rest, PartState::Hover, PartState::Pressed];
    div()
        .w(px(300.0))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .children(states.into_iter().enumerate().map(|(i, state)| {
            option_row(
                id("option", state.name()),
                i + 1,
                &format!("Option {}", state.name().to_lowercase()),
                "What this choice means",
                false,
                state,
                a,
            )
        }))
        .child(option_row(
            id("option", "ticked"),
            4,
            "Option ticked",
            "A multi-select row that is on",
            true,
            PartState::Rest,
            a,
        ))
}

fn small_parts(a: Appearance) -> Div {
    let names = [
        "design system",
        "task-status-check",
        "referral",
        "artifact-id-lookup",
        "wifi",
    ];
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .gap(px(8.0))
                .children(Status::ALL.map(|status| status_pill(status, status.word(), a))),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .children(names.map(|name| avatar(name, 30.0, Some(Status::Needs), Ink::Window, a)))
                .child(claude_mark(24.0, a)),
        )
        .child(div().w(px(220.0)).child(field("", "Search", false, a)))
        .child(
            div()
                .w(px(220.0))
                .child(field("focused", "Search", true, a)),
        )
}

impl Render for KitGallery {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let a = self.appearance(window);
        div()
            .size_full()
            .p(px(24.0))
            .fill(Ink::Window, a)
            .flex()
            .flex_col()
            .items_start()
            .gap(px(18.0))
            .child(label("Chat kit", TypeRole::Title, Ink::Text, a))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(24.0))
                    .child(section("Buttons", a, buttons(a)))
                    .child(section("Icons", a, icon_grid(a))),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(24.0))
                    .child(section("Session rows", a, rows(a)))
                    .child(section("Options", a, options(a))),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(24.0))
                    .child(section("Pills, avatars, field", a, small_parts(a)))
                    .child(section("Text editor", a, text_editors(&self.editors, a))),
            )
            .child(section("Scroll bar", a, scroll_demo(a)))
    }
}
