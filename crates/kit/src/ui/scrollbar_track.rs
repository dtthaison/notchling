use gpui::{point, px, Bounds, ListState, Pixels, ScrollHandle};

use crate::scrollbar::{self as math, list_scrolled, EDGE_INSET};

#[derive(Clone)]
pub(super) enum Track {
    Div(ScrollHandle),
    List(ListState),
}

impl Track {
    pub fn max_offset(&self) -> f32 {
        match self {
            Track::Div(handle) => f32::from(handle.max_offset().height),
            Track::List(state) => f32::from(state.max_offset_for_scrollbar().height),
        }
    }

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

pub(super) fn scroll_to(track: &Track, scrolled: f32) {
    match track {
        Track::Div(handle) => {
            let offset = handle.offset();
            handle.set_offset(point(offset.x, px(-scrolled)));
        }
        Track::List(state) => state.set_offset_from_scrollbar(point(px(0.0), px(scrolled))),
    }
}
