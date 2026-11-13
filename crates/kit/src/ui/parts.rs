use std::cell::Cell;

use gpui::{div, prelude::*, px, App, Div, ElementId, FontWeight, Global, Position, Stateful};

use super::icons::icon;
use super::widgets::button_with;
use super::{hsla, KitStyled, SYSTEM_FONT};
use crate::labels::{
    badge_place, button_state, Anchor, Corner, Labels, BADGE_HEIGHT, BADGE_PADDING, BADGE_TEXT,
    OUTLINE_ALPHA, OUTLINE_RADIUS, OUTLINE_WIDTH,
};
use crate::part::ButtonKind;
use crate::render_map::{label_inks, Region};
use crate::{Appearance, Icon, Ink};

impl Global for Labels {}

thread_local! {
    static REGION: Cell<Option<Region>> = const { Cell::new(None) };
}

#[must_use]
pub struct RegionScope(Option<Region>);

impl RegionScope {
    pub fn enter(region: Option<Region>) -> RegionScope {
        RegionScope(REGION.with(|r| r.replace(region)))
    }
}

impl Drop for RegionScope {
    fn drop(&mut self) {
        REGION.with(|r| r.set(self.0));
    }
}

fn current_inks() -> (Ink, Ink) {
    label_inks(REGION.with(Cell::get))
}

pub fn parts_on(cx: &App) -> bool {
    cx.try_global::<Labels>().is_some_and(|l| l.on)
}

pub fn set_parts(cx: &mut App, on: bool) {
    cx.default_global::<Labels>().on = on;
    cx.refresh_windows();
}

pub fn toggle_parts(cx: &mut App) -> bool {
    let on = cx.default_global::<Labels>().toggle();
    cx.refresh_windows();
    on
}

pub fn parts_tip(on: bool) -> &'static str {
    if on {
        "Hide the part names"
    } else {
        "Show the name of every part, to point at one when asking for a change"
    }
}

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

fn badge(name: &str, corner: Corner, inside: bool, appearance: Appearance) -> Div {
    let (fill, on) = current_inks();
    let place = badge_place(corner, inside);
    let pill = div()
        .h(px(BADGE_HEIGHT))
        .px(px(BADGE_PADDING))
        .rounded_full()
        .fill(fill, appearance)
        .ink(on, appearance)
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

pub trait KitPart: Styled + ParentElement + Sized {
    fn part(self, name: &str, appearance: Appearance, cx: &App) -> Self {
        self.part_at(name, Corner::TopLeading, appearance, cx)
    }

    fn part_at(self, name: &str, corner: Corner, appearance: Appearance, cx: &App) -> Self {
        self.tag_part(name, corner, false, appearance, cx)
    }

    fn part_inside(self, name: &str, corner: Corner, appearance: Appearance, cx: &App) -> Self {
        self.tag_part(name, corner, true, appearance, cx)
    }

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
        let (line, _) = current_inks();
        let outline = div()
            .absolute()
            .inset_0()
            .rounded(px(OUTLINE_RADIUS))
            .border(px(OUTLINE_WIDTH))
            .border_dashed()
            .border_color(hsla(line.color(appearance).with_alpha(OUTLINE_ALPHA)))
            .child(badge(name, corner, inside, appearance));
        self.child(outline)
    }
}

impl<T: Styled + ParentElement> KitPart for T {}
