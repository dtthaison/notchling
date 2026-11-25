use gpui::{div, prelude::*, px, AnyElement, CursorStyle, Div, Stateful, Window};

use crate::metric::{PANEL_EDGE_BEFORE, PANEL_EDGE_HIT, PANEL_EDGE_LINE};
use crate::ui::hsla;
use crate::{Appearance, Ink};

use crate::resize::{Axis, Zone};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopBorder {
    InContent,
    InTitleBar,
}

fn cursor(axis: Axis) -> CursorStyle {
    match axis {
        Axis::Horizontal => CursorStyle::ResizeLeftRight,
        Axis::Vertical => CursorStyle::ResizeUpDown,
        Axis::NorthWestSouthEast => CursorStyle::ResizeUpLeftDownRight,
        Axis::NorthEastSouthWest => CursorStyle::ResizeUpRightDownLeft,
    }
}

pub fn resize_edges(window: &Window, top: TopBorder) -> AnyElement {
    if window.is_fullscreen() || window.is_maximized() {
        return div().into_any_element();
    }
    let size = window.viewport_size();
    let (width, height) = (f32::from(size.width), f32::from(size.height));
    let zones = Zone::ALL.into_iter().filter_map(|zone| {
        let (x, y, w, h) = zone.frame(width, height, top == TopBorder::InContent)?;
        Some(
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .cursor(cursor(zone.axis())),
        )
    });
    div()
        .absolute()
        .top(px(0.0))
        .left(px(0.0))
        .size_full()
        .children(zones)
        .into_any_element()
}

pub fn panel_edge(id: &'static str, x: f32, active: bool, a: Appearance) -> Stateful<Div> {
    let line = hsla(Ink::Accent.color(a));
    div()
        .id(id)
        .group(id)
        .absolute()
        .top(px(0.0))
        .bottom(px(0.0))
        .left(px(x - PANEL_EDGE_BEFORE))
        .w(px(PANEL_EDGE_HIT))
        .flex()
        .cursor(cursor(Axis::Horizontal))
        .child(
            div()
                .h_full()
                .ml(px(PANEL_EDGE_BEFORE - PANEL_EDGE_LINE / 2.0))
                .w(px(PANEL_EDGE_LINE))
                .opacity(0.7)
                .when(active, move |d| d.bg(line))
                .group_hover(id, move |s| s.bg(line)),
        )
}
