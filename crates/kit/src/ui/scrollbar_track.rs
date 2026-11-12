//! What the overlay bar scrolls (`scrollbar`): a div's [`ScrollHandle`] or a GPUI [`ListState`],
//! and the kit's numbers for it this frame.

use gpui::{point, px, Bounds, ListState, Pixels, ScrollHandle};

use crate::scrollbar::{self as math, list_scrolled, EDGE_INSET};

/// The scrolling thing the bar follows and moves.
#[derive(Clone)]
pub(super) enum Track {
    Div(ScrollHandle),
    List(ListState),
}

impl Track {
    /// How far the content can scroll, in points, as measured so far (a list only measures the
    /// rows it has drawn).
    pub fn max_offset(&self) -> f32 {
        match self {
            Track::Div(handle) => f32::from(handle.max_offset().height),
            Track::List(state) => f32::from(state.max_offset_for_scrollbar().height),
        }
    }

    /// The thumb was grabbed: a list holds the height it reports to the bar still, so rows
    /// measured during the drag don't move the thumb away from the pointer.
    pub fn drag_started(&self) {
        if let Track::List(state) = self {
            state.scrollbar_drag_started();
        }
    }

    pub fn drag_ended(&self) {
        if let Track::List(state) = self {
            state.scrollbar_drag_ended();
        }
    }
}

/// The scroll container's numbers this frame, in the kit's terms (points scrolled down, not
/// GPUI's negative offset).
#[derive(Clone, Copy)]
pub(super) struct Geometry {
    pub track: f32,
    pub viewport: f32,
    pub content: f32,
    pub scrolled: f32,
}

impl Geometry {
    pub fn of(bounds: Bounds<Pixels>, track: &Track) -> Self {
        let viewport = f32::from(bounds.size.height);
        let max = track.max_offset();
        let scrolled = match track {
            Track::Div(handle) => -f32::from(handle.offset().y),
            Track::List(state) => {
                // A list that follows its end has no offset to read (`logical_scroll_top` is past
                // the last item); `list_scrolled` puts it at the end.
                let following = state.logical_scroll_top().item_ix >= state.item_count();
                list_scrolled(
                    following,
                    -f32::from(state.scroll_px_offset_for_scrollbar().y),
                    max,
                )
            }
        };
        Geometry {
            track: (viewport - 2.0 * EDGE_INSET).max(0.0),
            viewport,
            content: viewport + max,
            scrolled,
        }
    }

    pub fn thumb(self) -> Option<math::Thumb> {
        math::thumb(self.track, self.viewport, self.content, self.scrolled)
    }
}

/// Scrolls the container to `scrolled` points down (a div keeps its horizontal offset).
pub(super) fn scroll_to(track: &Track, scrolled: f32) {
    match track {
        Track::Div(handle) => {
            let offset = handle.offset();
            handle.set_offset(point(offset.x, px(-scrolled)));
        }
        Track::List(state) => state.set_offset_from_scrollbar(point(px(0.0), px(scrolled))),
    }
}
