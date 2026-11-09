//! Part labels over GPUI: the names of the parts on screen while the ? button is on, so a change
//! can be asked for by name ("the Context Meter"). The state is one [`Global`] ([`Labels`]) every
//! window reads, so Chat and the notch show their names together; a tagged element gets a dashed
//! guide outline and a name badge on its edge, drawn over it without taking layout space or
//! clicks (neither has a hitbox). The pure half (where the badge sits, the button's state) is
//! `crate::labels`. Ported from `KitPartLabel` and `KitPartsButton` in `ChatKit.swift`.

use gpui::{div, prelude::*, px, App, Div, ElementId, FontWeight, Global, Position, Stateful};

use super::icons::icon;
use super::widgets::button_with;
use super::{hsla, KitStyled, SYSTEM_FONT};
use crate::labels::{
    badge_place, button_state, Anchor, Corner, Labels, BADGE_HEIGHT, BADGE_PADDING, BADGE_TEXT,
    OUTLINE_ALPHA, OUTLINE_RADIUS, OUTLINE_WIDTH,
};
use crate::part::ButtonKind;
use crate::{Appearance, Icon, Ink};

impl Global for Labels {}

/// Whether the part names show.
pub fn parts_on(cx: &App) -> bool {
    cx.try_global::<Labels>().is_some_and(|l| l.on)
}

/// Shows or hides the part names in every window.
pub fn set_parts(cx: &mut App, on: bool) {
    cx.default_global::<Labels>().on = on;
    cx.refresh_windows();
}

/// Flips the part names; the new state.
pub fn toggle_parts(cx: &mut App) -> bool {
    let on = cx.default_global::<Labels>().toggle();
    cx.refresh_windows();
    on
}

/// The ? button's hover, for the caller to put on it with its tooltip.
pub fn parts_tip(on: bool) -> &'static str {
    if on {
        "Hide the part names"
    } else {
        "Show the name of every part, to point at one when asking for a change"
    }
}

/// The ? button: a ghost button, pressed and in the guide ink while the names show. The caller
/// adds the click (`toggle_parts`) and the tooltip (`parts_tip`).
pub fn parts_button(id: impl Into<ElementId>, appearance: Appearance, cx: &App) -> Stateful<Div> {
    let on = parts_on(cx);
    let ink = if on { Ink::Guide } else { Ink::TextMuted };
    button_with(
        id,
        icon(Icon::Unsure, 14.0, ink, appearance),
        ButtonKind::Ghost,
        button_state(on),
        appearance,
    )
}

/// The name badge: a guide capsule with the name in onGuide, placed by `badge_place`. Its wrapper
/// spans the outline's width so a centred badge can centre, and is positioned on the edge.
fn badge(name: &str, corner: Corner, inside: bool, appearance: Appearance) -> Div {
    let place = badge_place(corner, inside);
    let pill = div()
        .h(px(BADGE_HEIGHT))
        .px(px(BADGE_PADDING))
        .rounded_full()
        .fill(Ink::Guide, appearance)
        .ink(Ink::OnGuide, appearance)
        .font_family(SYSTEM_FONT)
        .text_size(px(BADGE_TEXT))
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .flex()
        .items_center()
        .child(name.to_owned());
    let row = div().absolute().left_0().right_0().flex();
    let row = match place.x {
        Anchor::Start(along) => row.justify_start().pl(px(along)),
        Anchor::Center => row.justify_center(),
        Anchor::End(along) => row.justify_end().pr(px(along)),
    };
    let row = match place.y {
        Anchor::Start(across) => row.top(px(across)),
        Anchor::Center => row.top_0().bottom_0().items_center(),
        Anchor::End(across) => row.bottom(px(across)),
    };
    row.child(pill)
}

/// Names an element for the ? button. Call it last in the chain, so the outline and badge paint
/// over the element's children.
pub trait KitPart: Styled + ParentElement + Sized {
    /// Names this part, the badge at its top left. Use the name the user will see; search the
    /// code for `.part("Name"` or `.part_at("Name"`.
    fn part(self, name: &str, appearance: Appearance, cx: &App) -> Self {
        self.part_at(name, Corner::TopLeading, appearance, cx)
    }

    /// Names this part with the badge on `corner`, straddling the outline.
    fn part_at(self, name: &str, corner: Corner, appearance: Appearance, cx: &App) -> Self {
        self.tag_part(name, corner, false, appearance, cx)
    }

    /// Names this part with the badge inside the outline, where the edge is clipped (a scroll
    /// view, the notch's rounded bottom).
    fn part_inside(self, name: &str, corner: Corner, appearance: Appearance, cx: &App) -> Self {
        self.tag_part(name, corner, true, appearance, cx)
    }

    /// The outline and the badge, when the names are on. The element becomes the containing block
    /// for them (`relative`) unless it is positioned already.
    fn tag_part(
        mut self,
        name: &str,
        corner: Corner,
        inside: bool,
        appearance: Appearance,
        cx: &App,
    ) -> Self {
        if !parts_on(cx) {
            return self;
        }
        if self.style().position.is_none() {
            self.style().position = Some(Position::Relative);
        }
        let outline = div()
            .absolute()
            .inset_0()
            .rounded(px(OUTLINE_RADIUS))
            .border(px(OUTLINE_WIDTH))
            .border_dashed()
            .border_color(hsla(Ink::Guide.color(appearance).with_alpha(OUTLINE_ALPHA)))
            .child(badge(name, corner, inside, appearance));
        self.child(outline)
    }
}

impl<T: Styled + ParentElement> KitPart for T {}
