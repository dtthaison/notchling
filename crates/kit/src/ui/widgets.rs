//! The kit's parts as GPUI elements. Each takes an [`Appearance`] and, where it has states, a
//! `state`: `PartState::Rest` makes it live (hover and press from the mouse, the pointing hand);
//! any other state is forced, which is how the gallery shows every state at once.

use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, Div, ElementId, FontWeight, Stateful,
};

use super::{hsla, KitStyled, SYSTEM_FONT};
use crate::ink::{avatar as avatar_colors, claude_mark_top};
use crate::metric::{
    AVATAR, BUTTON_PADDING, CONTROL, OPTION_BADGE, PILL_HEIGHT, RADIUS_CARD, RADIUS_MD, RADIUS_SM,
    SESSION_ROW,
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
        .child(title.to_owned());
    interactive(
        element,
        state,
        button_look(kind, PartState::Hover, appearance),
        button_look(kind, PartState::Pressed, appearance),
    )
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
    let element = div()
        .id(id)
        .w_full()
        .min_h(px(SESSION_ROW))
        .px(px(10.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .rounded(px(RADIUS_MD))
        .look(&row_look(state, focused, appearance))
        .child(avatar(name, AVATAR, Some(status), Ink::Sidebar, appearance))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap(px(2.0))
                .overflow_hidden()
                .child(label(name, TypeRole::Heading, Ink::Text, appearance))
                .child(label(
                    status_line,
                    TypeRole::Meta,
                    Ink::TextMuted,
                    appearance,
                )),
        )
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
        Icon::Check.glyph().to_owned()
    } else {
        number.to_string()
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
        .child(
            div()
                .text_size(px(10.0))
                .font_weight(FontWeight::MEDIUM)
                .child(Icon::status(status).glyph()),
        )
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
