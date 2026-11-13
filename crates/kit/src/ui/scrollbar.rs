use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    fill, point, px, size, AnyElement, App, Bounds, CursorStyle, DispatchPhase, Div, Element,
    ElementId, Global, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId,
    InteractiveElement, IntoElement, LayoutId, List, ListState, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, ScrollHandle, ScrollWheelEvent, Stateful,
    StatefulInteractiveElement, Window,
};

use super::scrollbar_track::{scroll_to, Geometry, Track};
use super::{hsla, window_appearance};
use crate::scrollbar::{self as math, EDGE_INSET, THUMB_WIDTH, THUMB_WIDTH_HOVER, ZONE_WIDTH};
use crate::Ink;

const THUMB_ALPHA: f32 = 0.55;
const THUMB_ALPHA_ACTIVE: f32 = 0.9;

pub struct PinScrollbars;

impl Global for PinScrollbars {}

pub fn scrollable(mut inner: Stateful<Div>) -> Scrollable {
    let id = inner
        .interactivity()
        .element_id
        .clone()
        .unwrap_or_else(|| ElementId::Name("scrollable".into()));
    Scrollable {
        id,
        inner: Some(Inner::Div(Box::new(inner))),
        handle: None,
        list: None,
        thumb: None,
    }
}

pub fn scrollable_list(id: impl Into<ElementId>, list: List, state: &ListState) -> Scrollable {
    Scrollable {
        id: id.into(),
        inner: Some(Inner::Any(list.into_any_element())),
        handle: None,
        list: Some(state.clone()),
        thumb: None,
    }
}

enum Inner {
    Div(Box<Stateful<Div>>),
    Any(AnyElement),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbAt {
    pub top: f32,
    pub bottom: f32,
    pub track_top: f32,
    pub track_bottom: f32,
}

pub type ThumbRecord = Rc<Cell<Option<ThumbAt>>>;

pub struct Scrollable {
    id: ElementId,
    inner: Option<Inner>,
    handle: Option<ScrollHandle>,
    list: Option<ListState>,
    thumb: Option<ThumbRecord>,
}

impl Scrollable {
    pub fn tracking(mut self, handle: &ScrollHandle) -> Self {
        self.handle = Some(handle.clone());
        self
    }

    pub fn record_thumb(mut self, record: &ThumbRecord) -> Self {
        self.thumb = Some(record.clone());
        self
    }
}

struct Bar {
    own: ScrollHandle,
    waking: bool,
    active: Option<Instant>,
    hovered: bool,
    drag: Option<Drag>,
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
    track: Track,
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
                        waking: false,
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
                waking: false,
                active: None,
                hovered: false,
                drag: None,
                shown: false,
            })),
        };
        let (mut inner, track) = match (self.inner.take(), &self.list) {
            (Some(Inner::Any(list)), Some(state)) => (list, Track::List(state.clone())),
            (Some(Inner::Div(div)), _) => {
                let handle = self
                    .handle
                    .clone()
                    .unwrap_or_else(|| bar.borrow().own.clone());
                (
                    (*div).track_scroll(&handle).into_any_element(),
                    Track::Div(handle),
                )
            }
            (_, _) => (
                gpui::div().into_any_element(),
                Track::Div(bar.borrow().own.clone()),
            ),
        };
        let layout = inner.request_layout(window, cx);
        (layout, Layout { inner, bar, track })
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
        let scrolls = layout.track.max_offset() > 0.5;
        let zone = scrolls.then(|| {
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
        let track = layout.track.clone();
        let bar = layout.bar.clone();
        let view = window.current_view();
        let geometry = Geometry::of(bounds, &track);
        if let Some(record) = &self.thumb {
            let track_top = f32::from(bounds.top()) + EDGE_INSET;
            record.set(geometry.thumb().map(|t| ThumbAt {
                top: track_top + t.start,
                bottom: track_top + t.end(),
                track_top,
                track_bottom: track_top + geometry.track,
            }));
        }
        let now = Instant::now();
        let appearance = window_appearance(window);
        let pinned = cx.has_global::<PinScrollbars>();

        let (alpha, hovered, dragging) = {
            let mut state = bar.borrow_mut();
            if pinned {
                state.active = Some(now);
            }
            let dragging = state.drag.is_some();
            let hovered =
                dragging || math::holds(zone.is_hovered(window), state.shown, state.hovered);
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
            if !hovered && state.shown && !pinned && !state.waking {
                if let Some(wait) = math::next_frame_in(since) {
                    state.waking = true;
                    let (bar, view) = (bar.clone(), window.current_view());
                    window
                        .spawn(cx, async move |cx| {
                            cx.background_executor()
                                .timer(Duration::from_secs_f32(wait))
                                .await;
                            bar.borrow_mut().waking = false;
                            cx.update(|_, cx| cx.notify(view)).ok();
                        })
                        .detach();
                }
            }
            (alpha, hovered, dragging)
        };

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

        {
            let (bar, track, zone) = (bar.clone(), track.clone(), zone.clone());
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let mut state = bar.borrow_mut();
                if let Some(drag) = state.drag {
                    if event.pressed_button == Some(MouseButton::Left) {
                        let geometry = Geometry::of(bounds, &track);
                        let moved = f32::from(event.position.y) - drag.pointer;
                        let to = math::scrolled_for_drag(
                            geometry.track,
                            geometry.viewport,
                            geometry.content,
                            drag.scrolled,
                            moved,
                        );
                        scroll_to(&track, to);
                    } else {
                        state.drag = None;
                        track.drag_ended();
                    }
                    cx.notify(view);
                    cx.stop_propagation();
                    return;
                }
                let over = math::holds(zone.is_hovered(window), state.shown, state.hovered);
                if over != state.hovered {
                    state.hovered = over;
                    if !over {
                        state.active = Some(Instant::now());
                    }
                    cx.notify(view);
                }
            });
        }
        {
            let (bar, track, zone) = (bar.clone(), track.clone(), zone.clone());
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
                let geometry = Geometry::of(bounds, &track);
                let y = f32::from(event.position.y) - f32::from(bounds.top()) - EDGE_INSET;
                match math::scrolled_for_page(
                    geometry.track,
                    geometry.viewport,
                    geometry.content,
                    geometry.scrolled,
                    y,
                ) {
                    Some(to) => scroll_to(&track, to),
                    None => {
                        track.drag_started();
                        state.drag = Some(Drag {
                            pointer: f32::from(event.position.y),
                            scrolled: geometry.scrolled,
                        });
                    }
                }
                state.active = Some(Instant::now());
                cx.notify(view);
                cx.stop_propagation();
            });
        }
        {
            let (bar, track, zone) = (bar.clone(), track.clone(), zone.clone());
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                let mut state = bar.borrow_mut();
                if state.drag.take().is_some() {
                    track.drag_ended();
                    state.hovered = zone.is_hovered(window);
                    state.active = Some(Instant::now());
                    cx.notify(view);
                }
            });
        }
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

fn strip(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds {
        origin: point(bounds.right() - px(ZONE_WIDTH), bounds.top()),
        size: size(px(ZONE_WIDTH), bounds.size.height),
    }
}
