use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, Div, ElementId, FontWeight, Stateful,
    StyleRefinement,
};

use super::icons::{icon, icon_in};
use super::tool_images::tool_image;
use super::{hsla, KitStyled, SYSTEM_FONT};
use crate::ink::{avatar as avatar_colors, claude_mark_top};
use crate::metric::{
    BUTTON_PADDING, CONTROL, OPTION_BADGE, PILL_HEIGHT, RADIUS_CARD, RADIUS_MD, RADIUS_SM,
    SESSION_LINE_HEADING, SESSION_LINE_META, SESSION_ROW, SESSION_ROW_ACCOUNT, TOOL_ICON,
};
use crate::part::{
    button_look, field_look, initials, option_look, row_look, ButtonKind, Look, PartState,
};
use crate::{Appearance, Icon, Ink, Status, TypeRole};

pub fn label(text: &str, role: TypeRole, ink: Ink, appearance: Appearance) -> Div {
    div()
        .kit_type(role)
        .ink(ink, appearance)
        .child(role.display(text))
}

pub fn card(appearance: Appearance) -> Div {
    div()
        .rounded(px(RADIUS_CARD))
        .fill(Ink::Card, appearance)
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(appearance)))
}

fn state_style(style: StyleRefinement, look: &Look, role: Option<TypeRole>) -> StyleRefinement {
    let style = match role {
        Some(role) => style.kit_type(role),
        None => style,
    };
    style.look(look)
}

fn interactive(
    element: Stateful<Div>,
    state: PartState,
    hover: Look,
    pressed: Look,
    role: Option<TypeRole>,
) -> Stateful<Div> {
    match state {
        PartState::Rest => element
            .cursor_pointer()
            .hover(move |style| state_style(style, &hover, role))
            .active(move |style| state_style(style, &pressed, role)),
        PartState::Disabled => element,
        _ => element.cursor_pointer(),
    }
}

pub fn button(
    id: impl Into<ElementId>,
    title: &str,
    kind: ButtonKind,
    state: PartState,
    appearance: Appearance,
) -> Stateful<Div> {
    button_with(id, title.to_owned(), kind, state, appearance)
}

pub fn button_with(
    id: impl Into<ElementId>,
    content: impl IntoElement,
    kind: ButtonKind,
    state: PartState,
    appearance: Appearance,
) -> Stateful<Div> {
    let element = div()
        .id(id)
        .h(px(CONTROL))
        .px(px(BUTTON_PADDING))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .kit_type(TypeRole::Label)
        .look(&button_look(kind, state, appearance))
        .child(content);
    interactive(
        element,
        state,
        button_look(kind, PartState::Hover, appearance),
        button_look(kind, PartState::Pressed, appearance),
        Some(TypeRole::Label),
    )
}

pub fn icon_button(
    id: impl Into<ElementId>,
    glyph: Icon,
    points: f32,
    kind: ButtonKind,
    state: PartState,
    appearance: Appearance,
) -> Stateful<Div> {
    let color = button_look(kind, state, appearance).text;
    button_with(id, icon_in(glyph, points, color), kind, state, appearance)
}

pub fn avatar(
    name: &str,
    size: f32,
    status: Option<Status>,
    ring: Ink,
    appearance: Appearance,
) -> Div {
    let (top, bottom) = avatar_colors(name);
    let mut element = div()
        .relative()
        .flex_shrink_0()
        .size(px(size))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(linear_gradient(
            135.0,
            linear_color_stop(hsla(top), 0.0),
            linear_color_stop(hsla(bottom), 1.0),
        ))
        .text_color(gpui::white())
        .font_family(SYSTEM_FONT)
        .text_size(px(size * 0.4))
        .font_weight(FontWeight::BOLD)
        .child(initials(name));
    if let Some(status) = status {
        let badge = size * 0.32;
        element = element.child(
            div()
                .absolute()
                .right(px(-2.0))
                .bottom(px(-2.0))
                .size(px(badge))
                .rounded_full()
                .fill(Ink::status(status), appearance)
                .border(px(2.0))
                .border_color(hsla(ring.color(appearance))),
        );
    }
    element
}

pub fn claude_mark(size: f32, appearance: Appearance) -> Div {
    div()
        .flex_shrink_0()
        .size(px(size))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(linear_gradient(
            135.0,
            linear_color_stop(hsla(claude_mark_top()), 0.0),
            linear_color_stop(hsla(Ink::Accent.color(appearance)), 1.0),
        ))
        .ink(Ink::OnAccent, appearance)
        .text_size(px(size * 0.5))
        .font_weight(FontWeight::MEDIUM)
        .child(Icon::Claude.glyph())
}

pub fn phone_mark(size: f32, ink: Ink, appearance: Appearance) -> Div {
    let color = hsla(ink.color(appearance));
    let stroke = (size / 9.0).max(1.0);
    div()
        .flex_shrink_0()
        .relative()
        .w(px((size * 0.58).round()))
        .h(px(size))
        .rounded(px(size * 0.2))
        .border(px(stroke))
        .border_color(color)
        .child(
            div()
                .absolute()
                .bottom(px(stroke * 1.5))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(div().size(px(stroke)).rounded_full().bg(color)),
        )
}

#[allow(clippy::too_many_arguments)]
pub fn session_row(
    id: impl Into<ElementId>,
    name: &str,
    status_line: &str,
    age: &str,
    status: Status,
    state: PartState,
    focused: bool,
    appearance: Appearance,
) -> Stateful<Div> {
    let lines = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(2.0))
        .overflow_hidden()
        .child(label(name, TypeRole::Heading, Ink::Text, appearance).elide())
        .child(label(status_line, TypeRole::Meta, Ink::TextMuted, appearance).elide());
    session_row_shell(
        id,
        div().min_h(px(SESSION_ROW)),
        name,
        lines,
        age,
        status,
        state,
        focused,
        appearance,
    )
}

fn marked_line(mark: impl IntoElement, indent: f32, text: Div) -> Div {
    text.relative()
        .pl(px(indent))
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .child(mark),
        )
        .elide()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolMark {
    Claude,
    Codex,
    Antigravity,
}

impl ToolMark {
    pub fn name(self) -> &'static str {
        match self {
            ToolMark::Claude => "Claude Code",
            ToolMark::Codex => "Codex",
            ToolMark::Antigravity => "Antigravity CLI",
        }
    }

    pub fn bundle_ids(self) -> &'static [&'static str] {
        match self {
            ToolMark::Claude => &["com.anthropic.claudefordesktop"],
            ToolMark::Codex => &["com.openai.codex"],
            ToolMark::Antigravity => &["com.google.antigravity", "com.google.antigravity-ide"],
        }
    }

    fn icon(self) -> Icon {
        match self {
            ToolMark::Claude => Icon::Claude,
            ToolMark::Codex => Icon::Code,
            ToolMark::Antigravity => Icon::Paperplane,
        }
    }
}

pub fn tool_icon(tool: ToolMark, size: f32, appearance: Appearance) -> Div {
    if let Some(image) = tool_image(tool) {
        return div()
            .flex_shrink_0()
            .size(px(size))
            .child(gpui::img(image).size(px(size)));
    }
    match tool {
        ToolMark::Claude => claude_mark(size, appearance),
        other => div()
            .flex_shrink_0()
            .size(px(size))
            .flex()
            .items_center()
            .justify_center()
            .child(icon(other.icon(), size * 0.75, Ink::TextMuted, appearance)),
    }
}

pub fn account_line(text: &str, lane: bool, appearance: Appearance) -> Div {
    marked_line(
        icon(
            if lane { Icon::Lane } else { Icon::Account },
            9.0,
            Ink::TextFaint,
            appearance,
        ),
        15.0,
        label(text, TypeRole::Meta, Ink::TextFaint, appearance).line_height(px(SESSION_LINE_META)),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn session_row_marked(
    id: impl Into<ElementId>,
    name: &str,
    tool: impl IntoElement,
    account: Option<impl IntoElement>,
    status_line: &str,
    age: &str,
    status: Status,
    state: PartState,
    focused: bool,
    appearance: Appearance,
) -> Stateful<Div> {
    let status_ink = if status == Status::Needs {
        Ink::AccentText
    } else {
        Ink::TextMuted
    };
    let who = marked_line(
        tool,
        TOOL_ICON + 5.0,
        label(name, TypeRole::Heading, Ink::Text, appearance).line_height(px(SESSION_LINE_HEADING)),
    );
    let lines = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(2.0))
        .overflow_hidden()
        .child(who)
        .children(account)
        .child(
            label(status_line, TypeRole::Meta, status_ink, appearance)
                .line_height(px(SESSION_LINE_META))
                .elide(),
        );
    session_row_shell(
        id,
        div().h(px(SESSION_ROW_ACCOUNT)),
        name,
        lines,
        age,
        status,
        state,
        focused,
        appearance,
    )
}

#[allow(clippy::too_many_arguments)]
fn session_row_shell(
    id: impl Into<ElementId>,
    frame: Div,
    _name: &str,
    lines: Div,
    age: &str,
    _status: Status,
    state: PartState,
    focused: bool,
    appearance: Appearance,
) -> Stateful<Div> {
    let element = frame
        .id(id)
        .w_full()
        .flex_shrink_0()
        .px(px(10.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .rounded(px(RADIUS_MD))
        .look(&row_look(state, focused, appearance))
        .child(lines)
        .child(label(age, TypeRole::Meta, Ink::TextFaint, appearance));
    interactive(
        element,
        state,
        row_look(PartState::Hover, focused, appearance),
        row_look(PartState::Pressed, focused, appearance),
        None,
    )
}

pub fn option_row(
    id: impl Into<ElementId>,
    number: usize,
    title: &str,
    detail: &str,
    selected: bool,
    state: PartState,
    appearance: Appearance,
) -> Stateful<Div> {
    let look = option_look(state, selected, appearance);
    let badge_text = if selected {
        icon_in(Icon::Check, 10.0, look.badge_text).into_any_element()
    } else {
        number.to_string().into_any_element()
    };
    let badge = div()
        .flex_shrink_0()
        .size(px(OPTION_BADGE))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(hsla(look.badge_fill))
        .text_color(hsla(look.badge_text))
        .kit_type(TypeRole::Label)
        .child(badge_text);
    let element = div()
        .id(id)
        .w_full()
        .p(px(10.0))
        .flex()
        .items_start()
        .gap(px(10.0))
        .rounded(px(RADIUS_MD))
        .look(&look.look)
        .child(badge)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap(px(2.0))
                .child(label(title, TypeRole::Heading, Ink::Text, appearance))
                .child(label(detail, TypeRole::Meta, Ink::TextMuted, appearance)),
        );
    interactive(
        element,
        state,
        option_look(PartState::Hover, selected, appearance).look,
        option_look(PartState::Pressed, selected, appearance).look,
        None,
    )
}

pub fn status_pill(status: Status, text: &str, appearance: Appearance) -> Div {
    let color = Ink::status(status).color(appearance);
    let text_color = if status == Status::Needs {
        Ink::AccentText.color(appearance)
    } else {
        color
    };
    div()
        .h(px(PILL_HEIGHT))
        .px(px(8.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .rounded_full()
        .bg(hsla(color.with_alpha(0.16)))
        .text_color(hsla(text_color))
        .child(icon_in(Icon::status(status), 10.0, text_color))
        .child(div().kit_type(TypeRole::Label).child(text.to_owned()))
}

pub fn field(text: &str, placeholder: &str, focused: bool, appearance: Appearance) -> Div {
    let empty = text.is_empty();
    div()
        .min_h(px(CONTROL))
        .px(px(10.0))
        .flex()
        .items_center()
        .rounded(px(RADIUS_SM))
        .kit_type(TypeRole::Body)
        .look(&field_look(focused, appearance))
        .when(empty, |field| field.ink(Ink::TextFaint, appearance))
        .child(if empty { placeholder } else { text }.to_owned())
}

#[cfg(test)]
mod press_tests {
    use super::*;

    #[test]
    fn a_pressed_button_keeps_its_text_size() {
        let look = button_look(ButtonKind::Ghost, PartState::Pressed, Appearance::Dark);
        let style = state_style(StyleRefinement::default(), &look, Some(TypeRole::Label));
        let text = style.text.expect("a text style");
        assert_eq!(text.font_size, Some(px(TypeRole::Label.spec().size).into()));
        assert!(text.color.is_some());
    }
}
