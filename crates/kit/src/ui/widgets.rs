//! The kit's parts as GPUI elements. Each takes an [`Appearance`] and, where it has states, a
//! `state`: `PartState::Rest` makes it live (hover and press from the mouse, the pointing hand);
//! any other state is forced, which is how the gallery shows every state at once.

use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, Div, ElementId, FontWeight, Stateful,
};

use super::icons::{icon, icon_in};
use super::tool_images::tool_image;
use super::{hsla, KitStyled, SYSTEM_FONT};
use crate::ink::{avatar as avatar_colors, claude_mark_top};
use crate::metric::{
    AVATAR, BUTTON_PADDING, CONTROL, OPTION_BADGE, PILL_HEIGHT, RADIUS_CARD, RADIUS_MD, RADIUS_SM,
    SESSION_LINE_HEADING, SESSION_LINE_META, SESSION_ROW, SESSION_ROW_ACCOUNT, TOOL_ICON,
};
use crate::part::{
    button_look, field_look, initials, option_look, row_look, ButtonKind, Look, PartState,
};
use crate::{Appearance, Icon, Ink, Status, TypeRole};

/// Text in a type role and an ink. Section headers come out uppercase.
pub fn label(text: &str, role: TypeRole, ink: Ink, appearance: Appearance) -> Div {
    div()
        .kit_type(role)
        .ink(ink, appearance)
        .child(role.display(text))
}

/// A card: fill, hairline, radius.
pub fn card(appearance: Appearance) -> Div {
    div()
        .rounded(px(RADIUS_CARD))
        .fill(Ink::Card, appearance)
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(appearance)))
}

/// Live hover and press looks on an element at rest; a forced state draws only itself. The pointing
/// hand shows over anything clickable that isn't disabled (LINT-POINTER-CURSOR's rule, kept here).
fn interactive(
    element: Stateful<Div>,
    state: PartState,
    hover: Look,
    pressed: Look,
) -> Stateful<Div> {
    match state {
        PartState::Rest => element
            .cursor_pointer()
            .hover(move |style| style.look(&hover))
            .active(move |style| style.look(&pressed)),
        PartState::Disabled => element,
        _ => element.cursor_pointer(),
    }
}

/// Spec: Button. Height 28, padding 12, radius sm, label type.
pub fn button(
    id: impl Into<ElementId>,
    title: &str,
    kind: ButtonKind,
    state: PartState,
    appearance: Appearance,
) -> Stateful<Div> {
    button_with(id, title.to_owned(), kind, state, appearance)
}

/// A button around any element (a drawn mark such as [`phone_mark`]) instead of a title.
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
    )
}

/// A button that holds an icon of `points` in its kind's text color: close, back, add.
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

/// Spec: Avatar. Initials on the session's own gradient, with an optional status badge ringed in
/// the surface it sits on.
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

/// Claude's mark: a sparkle on an orange disc.
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

/// A phone, drawn (`Icon::Phone`, SF `iphone`): a rounded outline `size` tall and a dot for the
/// home button, in an ink. The text glyphs for a phone come out thin or tiny in the system font.
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

/// Spec: Session row. Avatar 30 with a status badge, name, status line, age. Height 52, radius md.
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
        // One line each, clipped with an ellipsis, so every row keeps the same height.
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

/// `text` with `mark` in a margin `indent` wide, cut with an ellipsis. The mark is placed over the
/// text's own box, not laid out beside it: `elide` needs the text to be a direct child of the
/// lines column, which gives it a definite width. Wrapped in another box (a row, or a column of
/// its own) the text is measured narrow and shows only its first word.
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

/// Which tool runs a session, for [`tool_icon`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolMark {
    Claude,
    Codex,
    Antigravity,
}

impl ToolMark {
    /// The tool's name, for a tooltip or a label.
    pub fn name(self) -> &'static str {
        match self {
            ToolMark::Claude => "Claude Code",
            ToolMark::Codex => "Codex",
            ToolMark::Antigravity => "Antigravity CLI",
        }
    }

    /// The installed apps whose icon stands for the tool, the first found wins (the CLIs have no
    /// icon of their own): the Swift `SessionTool.bundleIDs`.
    pub fn bundle_ids(self) -> &'static [&'static str] {
        match self {
            ToolMark::Claude => &["com.anthropic.claudefordesktop"],
            ToolMark::Codex => &["com.openai.codex"],
            ToolMark::Antigravity => &["com.google.antigravity", "com.google.antigravity-ide"],
        }
    }

    /// The Swift `SessionTool.symbol`: what it draws when the tool's app isn't installed.
    fn icon(self) -> Icon {
        match self {
            ToolMark::Claude => Icon::Claude,
            ToolMark::Codex => Icon::Code,
            ToolMark::Antigravity => Icon::Paperplane,
        }
    }
}

/// Spec: Tool icon (the Swift `ToolIcon`), `size` square: the tool's installed app icon, else
/// Claude's mark, else the tool's symbol at three quarters of the box in muted ink, as the Swift
/// app draws it with no app icon to hand.
/// The OpenAI and Google marks are brand artwork and aren't copied here.
pub fn tool_icon(tool: ToolMark, size: f32, appearance: Appearance) -> Div {
    // The installed app's own icon when the app has handed it over (`tool_images`).
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

/// Spec: a session row's account line: a 9 pt person (the main login) or branch (a lane) and the
/// email, meta type in faint ink, clipped with an ellipsis.
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

/// Spec: Session row, the sidebar's: three lines, who (the tool icon and the name), on which
/// account, doing what; the status line is accent text while the session needs you. `tool` and
/// `account` are built by the caller (`tool_icon`, `account_line`) so it can name them as parts.
/// Fixed at [`SESSION_ROW_ACCOUNT`], with or without an account line.
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

/// What the two session rows share: the frame, the avatar, the lines column, the age.
#[allow(clippy::too_many_arguments)]
fn session_row_shell(
    id: impl Into<ElementId>,
    frame: Div,
    name: &str,
    lines: Div,
    age: &str,
    status: Status,
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
        .child(avatar(name, AVATAR, Some(status), Ink::Sidebar, appearance))
        .child(lines)
        .child(label(age, TypeRole::Meta, Ink::TextFaint, appearance));
    interactive(
        element,
        state,
        row_look(PartState::Hover, focused, appearance),
        row_look(PartState::Pressed, focused, appearance),
    )
}

/// Spec: Option, a question's answer. Number badge (the terminal's digit), title, detail.
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
    )
}

/// Spec: Status pill. Capsule, status color at 16%, status-colored icon and word, never color alone.
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

/// Spec: Field. A plain field with the kit's fill and focus border; shows the placeholder faint
/// while empty. Typing arrives with U1.
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
