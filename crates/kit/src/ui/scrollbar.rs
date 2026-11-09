//! A macOS-style overlay scroll bar, since GPUI 0.2.2's `overflow_y_scroll()` draws none.
//!
//! `scrollable(div().id("list").overflow_y_scroll()...)` is that same scrolling div, with the bar
//! painted over its right edge. It is the same layout node (no wrapper, so the div's `flex_1`,
//! `h`, padding and the rest keep meaning what they did) and takes no room of its own.
//!
//! - The thumb is thin and rounded, 7 pt wide and 9 under the pointer, 2 pt in from the edge, with
//!   no track behind it. It shows only while the content scrolls and fades about a second after it
//!   stops; the pointer over a shown bar keeps it (so it can be grabbed), but never shows a hidden
//!   one. Frames are drawn for the fade only while the bar is visible.
//! - Dragging the thumb scrolls; a press in the track pages toward it. Both take the press, so the
//!   content under the 14 pt strip doesn't also get it, and the pointer stays an arrow there. While
//!   the bar is hidden its strip passes presses and the pointer through; wheel scrolling over it
//!   always reaches the list.
//! - Hidden when the content fits.
//!
//! The scroll position is the container's [`ScrollHandle`]: pass the view's own with
//! [`Scrollable::tracking`] (which also does `track_scroll`), or leave it out and the bar keeps one in
//! element state under the div's id. The numbers are in [`crate::scrollbar`].

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    fill, point, px, size, AnyElement, App, Bounds, CursorStyle, DispatchPhase, Div, Element,
    ElementId, Global, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId,
    InteractiveElement, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, ScrollHandle, ScrollWheelEvent, Stateful, StatefulInteractiveElement,
    Window,
};

use super::{hsla, window_appearance};
use crate::scrollbar::{self as math, EDGE_INSET, THUMB_WIDTH, THUMB_WIDTH_HOVER, ZONE_WIDTH};
use crate::Ink;

/// The thumb at rest and while its bar is hovered or dragged: the ink's alpha.
const THUMB_ALPHA: f32 = 0.55;
const THUMB_ALPHA_ACTIVE: f32 = 0.9;

/// Draws every bar as if it had just scrolled, for a render (a still frame has no scroll). Set by the
/// render path (`NOTCHLING_RENDER_SCROLLBARS=1`), never by the app.
pub struct PinScrollbars;

impl Global for PinScrollbars {}

/// The scrolling div `inner` (an `id`, `overflow_y_scroll()` and whatever sizing it had) with the
/// overlay bar on top.
pub fn scrollable(mut inner: Stateful<Div>) -> Scrollable {
    let id = inner
        .interactivity()
        .element_id
        .clone()
        .unwrap_or_else(|| ElementId::Name("scrollable".into()));
    Scrollable {
        id,
        inner: Some(inner),
        handle: None,
    }
}

pub struct Scrollable {
    id: ElementId,
    inner: Option<Stateful<Div>>,
    handle: Option<ScrollHandle>,
}

impl Scrollable {
    /// Follows (and moves) this handle, the view's own: for a view that scrolls it from code or
    /// reads its offset. Replaces the div's own `track_scroll`.
    pub fn tracking(mut self, handle: &ScrollHandle) -> Self {
        self.handle = Some(handle.clone());
        self
    }
}

/// What the bar remembers between frames, in element state under the div's id.
struct Bar {
    /// The handle when the view gave none.
    own: ScrollHandle,
    /// Where the content was last frame (points scrolled down), to see it move.
    scrolled: Option<f32>,
    /// When it last moved, or the pointer last left.
    active: Option<Instant>,
    hovered: bool,
    drag: Option<Drag>,
    /// Whether the bar was on screen last frame: it takes presses only then.
    shown: bool,
}

#[derive(Clone, Copy)]
struct Drag {
    pointer: f32,
    scrolled: f32,
}

type Shared = Rc<RefCell<Bar>>;

pub struct Layout {
    inner: AnyElement,
    bar: Shared,
    handle: ScrollHandle,
}

pub struct Prepainted {
    zone: Option<Hitbox>,
}

impl IntoElement for Scrollable {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Scrollable {
    type RequestLayoutState = Layout;
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Layout) {
        let bar = match id {
            Some(id) => window.with_element_state(id, |state: Option<Shared>, _| {
                let bar = state.unwrap_or_else(|| {
                    Rc::new(RefCell::new(Bar {
                        own: ScrollHandle::new(),
                        scrolled: None,
                        active: None,
                        hovered: false,
                        drag: None,
                        shown: false,
                    }))
                });
                (bar.clone(), bar)
            }),
            None => Rc::new(RefCell::new(Bar {
                own: ScrollHandle::new(),
                scrolled: None,
                active: None,
                hovered: false,
                drag: None,
                shown: false,
            })),
        };
        let handle = self
            .handle
            .clone()
            .unwrap_or_else(|| bar.borrow().own.clone());
        let mut inner = self
            .inner
            .take()
            .map(|inner| inner.track_scroll(&handle).into_any_element())
            .unwrap_or_else(|| gpui::div().into_any_element());
        let layout = inner.request_layout(window, cx);
        (layout, Layout { inner, bar, handle })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Layout,
        window: &mut Window,
        cx: &mut App,
    ) -> Prepainted {
        layout.inner.prepaint(window, cx);
        // The div has measured its content by now.
        let scrolls = f32::from(layout.handle.max_offset().height) > 0.5;
        let zone = scrolls.then(|| {
            // The strip passes presses and the pointer through while the bar is hidden.
            let behavior = if layout.bar.borrow().shown {
                HitboxBehavior::BlockMouseExceptScroll
            } else {
                HitboxBehavior::Normal
            };
            window.insert_hitbox(strip(bounds), behavior)
        });
        Prepainted { zone }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Layout,
        prepaint: &mut Prepainted,
        window: &mut Window,
        cx: &mut App,
    ) {
        layout.inner.paint(window, cx);
        let Some(zone) = prepaint.zone.clone() else {
            layout.bar.borrow_mut().shown = false;
            return;
        };
        let handle = layout.handle.clone();
        let bar = layout.bar.clone();
        let geometry = Geometry::of(bounds, &handle);
        let now = Instant::now();
        let appearance = window_appearance(window);
        let pinned = cx.has_global::<PinScrollbars>();

        let (alpha, hovered, dragging) = {
            let mut state = bar.borrow_mut();
            let scrolled = geometry.scrolled;
            if state
                .scrolled
                .is_some_and(|last| (last - scrolled).abs() > 0.01)
            {
                state.active = Some(now);
            }
            state.scrolled = Some(scrolled);
            if pinned {
                state.active = Some(now);
            }
            let dragging = state.drag.is_some();
            // The pointer only holds a bar that scrolling already showed (so it can be grabbed);
            // over a hidden bar it shows nothing.
            let hovered = dragging || (zone.is_hovered(window) && (state.shown || state.hovered));
            if hovered != state.hovered {
                state.hovered = hovered;
                if !hovered {
                    state.active = Some(now);
                }
            }
            let since = state
                .active
                .map_or(f32::MAX, |at| now.duration_since(at).as_secs_f32());
            let alpha = if hovered {
                1.0
            } else {
                math::visibility(since)
            };
            state.shown = alpha > 0.0;
            // Frames only while the bar is on screen and still changing.
            if !hovered && state.shown && math::is_animating(since) && !pinned {
                window.request_animation_frame();
            }
            (alpha, hovered, dragging)
        };

        // The pointer stays an arrow over the bar, whatever is beneath it.
        window.set_cursor_style(CursorStyle::Arrow, &zone);

        if alpha > 0.0 {
            if let Some(thumb) = geometry.thumb() {
                let ink = Ink::TextMuted.color(appearance);
                let width = if hovered || dragging {
                    THUMB_WIDTH_HOVER
                } else {
                    THUMB_WIDTH
                };
                let top = f32::from(bounds.top()) + EDGE_INSET + thumb.start;
                let rect = Bounds {
                    origin: point(bounds.right() - px(EDGE_INSET + width), px(top)),
                    size: size(px(width), px(thumb.len)),
                };
                let a = if hovered || dragging {
                    THUMB_ALPHA_ACTIVE
                } else {
                    THUMB_ALPHA
                } * alpha;
                window
                    .paint_quad(fill(rect, hsla(ink.with_alpha(a))).corner_radii(px(width / 2.0)));
            }
        }

        // The pointer moving: hover, and the thumb following a drag.
        {
            let (bar, handle, zone) = (bar.clone(), handle.clone(), zone.clone());
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let mut state = bar.borrow_mut();
                if let Some(drag) = state.drag {
                    if event.pressed_button == Some(MouseButton::Left) {
                        let geometry = Geometry::of(bounds, &handle);
                        let moved = f32::from(event.position.y) - drag.pointer;
                        let to = math::scrolled_for_drag(
                            geometry.track,
                            geometry.viewport,
                            geometry.content,
                            drag.scrolled,
                            moved,
                        );
                        scroll_to(&handle, to);
                    } else {
                        state.drag = None;
                    }
                    window.refresh();
                    cx.stop_propagation();
                    return;
                }
                let over = zone.is_hovered(window);
                if over != state.hovered {
                    state.hovered = over;
                    if !over {
                        state.active = Some(Instant::now());
                    }
                    window.refresh();
                }
            });
        }
        // A press: on the thumb it grabs it, in the track it pages. The content never sees it.
        {
            let (bar, handle, zone) = (bar.clone(), handle.clone(), zone.clone());
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !zone.is_hovered(window)
                {
                    return;
                }
                let mut state = bar.borrow_mut();
                if !state.shown && !state.hovered {
                    return;
                }
                let geometry = Geometry::of(bounds, &handle);
                let y = f32::from(event.position.y) - f32::from(bounds.top()) - EDGE_INSET;
                match math::scrolled_for_page(
                    geometry.track,
                    geometry.viewport,
                    geometry.content,
                    geometry.scrolled,
                    y,
                ) {
                    Some(to) => scroll_to(&handle, to),
                    None => {
                        state.drag = Some(Drag {
                            pointer: f32::from(event.position.y),
                            scrolled: geometry.scrolled,
                        });
                    }
                }
                state.active = Some(Instant::now());
                window.refresh();
                cx.stop_propagation();
            });
        }
        {
            let (bar, zone) = (bar.clone(), zone.clone());
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, _| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                let mut state = bar.borrow_mut();
                if state.drag.take().is_some() {
                    state.hovered = zone.is_hovered(window);
                    state.active = Some(Instant::now());
                    window.refresh();
                }
            });
        }
        // A wheel over the list counts as scrolling even when it is already at an end.
        {
            let bar = bar.clone();
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, _, _| {
                if phase == DispatchPhase::Bubble && bounds.contains(&event.position) {
                    bar.borrow_mut().active = Some(Instant::now());
                }
            });
        }
    }
}

/// The bar's strip: along the container's right edge, its whole height.
fn strip(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds {
        origin: point(bounds.right() - px(ZONE_WIDTH), bounds.top()),
        size: size(px(ZONE_WIDTH), bounds.size.height),
    }
}

/// The scroll container's numbers this frame, in the kit's terms (points scrolled down, not
/// GPUI's negative offset).
#[derive(Clone, Copy)]
struct Geometry {
    track: f32,
    viewport: f32,
    content: f32,
    scrolled: f32,
}

impl Geometry {
    fn of(bounds: Bounds<Pixels>, handle: &ScrollHandle) -> Self {
        let viewport = f32::from(bounds.size.height);
        Geometry {
            track: (viewport - 2.0 * EDGE_INSET).max(0.0),
            viewport,
            content: viewport + f32::from(handle.max_offset().height),
            scrolled: -f32::from(handle.offset().y),
        }
    }

    fn thumb(self) -> Option<math::Thumb> {
        math::thumb(self.track, self.viewport, self.content, self.scrolled)
    }
}

/// Scrolls the container to `scrolled` points down, keeping its horizontal offset.
fn scroll_to(handle: &ScrollHandle, scrolled: f32) {
    let offset = handle.offset();
    handle.set_offset(point(offset.x, px(-scrolled)));
}
