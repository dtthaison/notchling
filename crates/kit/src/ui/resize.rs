//! The resize cursor over a resizable window's border. GPUI 0.2.2 sets the arrow cursor after every
//! draw of the active window when no element asks for one, which covers AppKit's own resize cursor
//! at the border; these zones ask for it again. They carry no mouse handlers and don't occlude, so
//! a press on the border still reaches AppKit's live resize (and the traffic lights, which are
//! AppKit's too). The geometry is `crate::resize`.

use gpui::{div, prelude::*, px, AnyElement, CursorStyle, Window};

use crate::resize::{Axis, Zone};

/// Whether the window's top border is part of what the content draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopBorder {
    /// A hidden (transparent) title bar: the content reaches the top of the window, so the top
    /// border has zones.
    InContent,
    /// A normal title bar sits above the content; its resize border isn't ours to mark.
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

/// The invisible border zones of a resizable window, to add as the last child of the window's
/// root (so they ask for the cursor over everything, but only on the border). Nothing while the
/// window is full screen or zoomed, which don't resize by the border. Only for windows opened
/// resizable.
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
