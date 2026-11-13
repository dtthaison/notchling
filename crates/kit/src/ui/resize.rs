use gpui::{div, prelude::*, px, AnyElement, CursorStyle, Window};

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
