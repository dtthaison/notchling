//! Icons drawn: the kit's Lucide SVGs (`crate::icon`) through GPUI's asset source, in an ink.

use std::borrow::Cow;
use std::f32::consts::TAU;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, radians, svg, AnyElement, AnyView, App, AssetSource, Bounds, Context, Div,
    Element, ElementId, Entity, EntityId, FontWeight, Global, GlobalElementId, InspectorElementId,
    LayoutId, Pixels, SharedString, Transformation, Window,
};

use super::hsla;
use crate::icon::{asset, assets, svg_box};
use crate::{Appearance, Color, Icon, Ink};

/// Serves the embedded icons. An app hands it to `Application::with_assets`, or `svg()` draws
/// nothing.
pub struct KitAssets;

impl AssetSource for KitAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(asset(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(assets(path).map(SharedString::from).collect())
    }
}

/// An icon the size of an SF Symbol of `points`, in `ink`: its Lucide SVG, else its text glyph.
pub fn icon(icon: Icon, points: f32, ink: Ink, a: Appearance) -> Div {
    icon_in(icon, points, ink.color(a))
}

/// [`icon`] in a color a part's look already chose (a badge's text, a button's). An SVG takes its
/// color from its own style, not from the text around it, so an icon inside a button needs this.
pub fn icon_in(icon: Icon, points: f32, color: Color) -> Div {
    let holder = div().flex_shrink_0().flex().items_center().justify_center();
    match icon.svg() {
        Some(path) => {
            let size = px(svg_box(points));
            holder
                .size(size)
                .child(svg().path(path).size(size).text_color(hsla(color)))
        }
        None => holder
            .text_size(px(points))
            .font_weight(FontWeight::MEDIUM)
            .text_color(hsla(color))
            .child(icon.glyph()),
    }
}

/// How long a spinner takes to turn once (the macOS ring turns about once a second).
const SPIN: Duration = Duration::from_secs(1);
/// How often a spinner moves on: ten times a second (36 degrees a step), not once per display
/// frame. GPUI draws
/// every view above a spinner again for each of its frames, so a spinner asking for the display's
/// 60 or 120 a second kept a whole window drawing (Chat with a working session took a core).
const SPIN_FRAME: Duration = Duration::from_millis(100);
/// Lucide's `loader-circle` fills 21 of its 24 units, so its box is this much bigger than the
/// spinner.
const SPIN_BOX: f32 = 1.2;

/// How far round a spinner is `t` (0 to 1) of the way through its turn, in turns. Smooth, not
/// stepped: a ring turned in fixed steps looks like it jumps, and the old eight-spoke loader
/// turned in 45 degree steps looked the same on every frame, so it never seemed to move.
fn spin_turns(t: f32) -> f32 {
    t.clamp(0.0, 1.0) % 1.0
}

/// Where every spinner is `since` the clock they share started, in turns: they all turn
/// together, and a spinner drawn by another redraw (a hover, a scroll) is where the clock says.
pub(super) fn turns_at(since: Duration) -> f32 {
    let turn = SPIN.as_nanos();
    spin_turns((since.as_nanos() % turn) as f32 / turn as f32)
}

/// How long until the next spinner frame, on one grid for every spinner, so several spinners on
/// screen move in the same frame rather than each asking for its own.
pub(super) fn until_next_frame(since: Duration) -> Duration {
    let frame = SPIN_FRAME.as_nanos();
    Duration::from_nanos((frame - since.as_nanos() % frame) as u64)
}

/// The clock every spinner reads.
pub(super) fn spin_clock() -> Duration {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed()
}

/// The loading spinner (the Swift `ProgressView().controlSize(...)`: 10 pt mini, 16 pt small):
/// Lucide's `loader-circle`, an open ring in `ink` turning once a second, moved on ten times a
/// second. `id` keeps the spinner's own small view between frames, so two spinners in one
/// place need two ids. It turns only while drawn: nothing asks for its next frame once it's gone.
pub fn spinner(id: impl Into<ElementId>, diameter: f32, ink: Ink, a: Appearance) -> Div {
    let size = px(diameter * SPIN_BOX);
    div()
        .flex_shrink_0()
        .size(size)
        .flex()
        .items_center()
        .justify_center()
        .child(Spin {
            id: id.into(),
            size,
            color: ink.color(a),
        })
}

/// Spinners waiting for the next frame, woken together by one timer: two timers for the same
/// moment woke apart and drew two frames.
#[derive(Default)]
struct Ticker {
    waiting: Vec<EntityId>,
    timer: bool,
}

impl Global for Ticker {}

/// Wakes the view `view` (a spinner, or a layer of them) at the next spinner frame, with every
/// other spinner that asked.
pub(super) fn next_spin_frame(cx: &mut App, view: EntityId) {
    let ticker = cx.default_global::<Ticker>();
    if !ticker.waiting.contains(&view) {
        ticker.waiting.push(view);
    }
    if std::mem::replace(&mut ticker.timer, true) {
        return;
    }
    let wait = until_next_frame(spin_clock());
    cx.spawn(async move |cx| {
        cx.background_executor().timer(wait).await;
        cx.update(|cx| {
            let ticker = cx.default_global::<Ticker>();
            ticker.timer = false;
            for view in std::mem::take(&mut ticker.waiting) {
                cx.notify(view);
            }
        })
        .ok();
    })
    .detach();
}

/// The ring itself, a view of its own: its frame wakes only it, so the views above it draw again
/// (GPUI redraws a view's ancestors) but nothing beside them does, and a view that observes
/// another (Chat's conversation, cached apart from the window) isn't told the window changed.
struct Turning {
    size: Pixels,
    color: Color,
}

impl Render for Turning {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let since = spin_clock();
        let id = cx.entity_id();
        next_spin_frame(cx, id);
        svg()
            .path(Icon::Spinner.svg().unwrap_or("icons/loader-circle.svg"))
            .size(self.size)
            .text_color(hsla(self.color))
            .with_transformation(Transformation::rotate(radians(turns_at(since) * TAU)))
    }
}

/// Puts the spinner's [`Turning`] view in the tree, kept in element state under its id.
struct Spin {
    id: ElementId,
    size: Pixels,
    color: Color,
}

impl IntoElement for Spin {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Spin {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

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
    ) -> (LayoutId, AnyElement) {
        let (size, color) = (self.size, self.color);
        // In a host drawn cached, a slot the host's layer paints (`spin_layer`), so the host
        // isn't drawn again for each frame of the spinner.
        if let Some(slots) = super::spin_layer::slots_here(window, cx) {
            let mut element = super::spin_layer::Slot { slots, size, color }.into_any_element();
            return (element.request_layout(window, cx), element);
        }
        let new = |cx: &mut App| cx.new(|_| Turning { size, color });
        let view = match id {
            Some(id) => window.with_element_state(id, |kept: Option<Entity<Turning>>, _| {
                let view = kept.unwrap_or_else(|| new(cx));
                (view.clone(), view)
            }),
            None => new(cx),
        };
        // Its look follows the caller's (an appearance switch); not a redraw of its own.
        view.update(cx, |turning, _| {
            turning.size = size;
            turning.color = color;
        });
        let mut element = AnyView::from(view).into_any_element();
        (element.request_layout(window, cx), element)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spinner_turns_smoothly_and_ends_where_it_began() {
        assert_eq!(spin_turns(0.0), 0.0);
        assert_eq!(spin_turns(0.125), 0.125);
        assert!(spin_turns(0.13) > spin_turns(0.125), "no 45 degree steps");
        assert_eq!(spin_turns(0.5), 0.5);
        assert_eq!(spin_turns(1.0), 0.0, "the turn ends where the next begins");
    }

    #[test]
    fn every_spinner_reads_one_clock() {
        assert_eq!(turns_at(Duration::ZERO), 0.0);
        assert!((turns_at(SPIN / 4) - 0.25).abs() < 1e-4);
        assert!(
            (turns_at(SPIN * 3 + SPIN / 2) - 0.5).abs() < 1e-4,
            "whole turns drop"
        );
    }

    #[test]
    fn spinners_wait_for_the_same_frame_ten_times_a_second() {
        assert_eq!(SPIN_FRAME.as_millis(), 100);
        assert_eq!(
            until_next_frame(Duration::ZERO),
            SPIN_FRAME,
            "on a frame: the next one"
        );
        let a = until_next_frame(Duration::from_millis(20));
        let b = until_next_frame(Duration::from_millis(50));
        assert_eq!(
            Duration::from_millis(20) + a,
            Duration::from_millis(50) + b,
            "two spinners at different moments wake together"
        );
        assert_eq!(
            until_next_frame(Duration::from_millis(130)),
            Duration::from_millis(70)
        );
    }
}
