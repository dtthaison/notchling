//! Spinners inside a cached view, drawn by a layer outside it. GPUI draws a notified view's
//! ancestors again, so a spinner that is a view of its own (`icons::spinner`) inside a view drawn
//! with `AnyView::cached` makes that view lay out and paint everything again on each of its ten
//! frames a second. In a host made with [`host_spinners`], a spinner leaves a slot instead (where it
//! is, how it is clipped, its ink), and one [`SpinLayer`] after the hosts in the window's tree
//! paints every slot and wakes only itself: the hosts' cached drawing stays.
//!
//! A host's slots are cleared when it draws again ([`SpinSlots::clear`], at the start of its
//! render); a host drawn from its cache keeps them, since its layout is the same. A host's element
//! goes in the tree through [`spin_host`], which says the host is on screen this frame, so a host
//! that is left out (a hidden panel) leaves no spinners behind.

use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::TAU;
use std::rc::Rc;

use gpui::{
    prelude::*, px, radians, AnyElement, AnyView, App, Bounds, ContentMask, Context, Element,
    ElementId, EntityId, Global, GlobalElementId, InspectorElementId, LayoutId, Negate, Pixels,
    Position, Style, TransformationMatrix, Window,
};

use super::hsla;
use super::icons::{next_spin_frame, spin_clock, turns_at};
use crate::{Color, Icon};

/// Where one spinner is drawn: its box in the window, the clip it was laid out under, its ink.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpinSlot {
    pub bounds: Bounds<Pixels>,
    pub mask: Bounds<Pixels>,
    pub color: Color,
}

impl SpinSlot {
    /// Whether any of it shows: a row scrolled out of its list leaves a slot clipped away.
    pub fn shows(&self) -> bool {
        let shown = self.bounds.intersect(&self.mask);
        shown.size.width > px(0.0) && shown.size.height > px(0.0)
    }
}

#[derive(Default)]
struct Slots {
    slots: Vec<SpinSlot>,
    /// The host's element was laid out this frame (`spin_host`); the layer takes it back.
    on_screen: bool,
}

/// One host's spinners. Cheap to clone: the host and the layer share it.
#[derive(Clone, Default)]
pub struct SpinSlots(Rc<RefCell<Slots>>);

impl SpinSlots {
    /// The host is drawing again: its spinners are about to be laid out anew.
    pub fn clear(&self) {
        self.0.borrow_mut().slots.clear();
    }

    pub fn push(&self, slot: SpinSlot) {
        self.0.borrow_mut().slots.push(slot);
    }

    /// How many slots the host holds: a spinner laid out twice in a frame (a list's autoscroll
    /// retry lays its rows out again) leaves two.
    pub fn len(&self) -> usize {
        self.0.borrow().slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The slots to paint this frame (none if the host wasn't in the tree), and marks the host
    /// as not seen for the next frame.
    pub fn take_shown(&self) -> Vec<SpinSlot> {
        let mut s = self.0.borrow_mut();
        let on_screen = std::mem::take(&mut s.on_screen);
        if on_screen {
            s.slots.iter().copied().filter(SpinSlot::shows).collect()
        } else {
            Vec::new()
        }
    }

    fn seen(&self) {
        self.0.borrow_mut().on_screen = true;
    }
}

/// Which views are spinner hosts.
#[derive(Default)]
struct Hosts(HashMap<EntityId, SpinSlots>);

impl Global for Hosts {}

/// Makes the view `host` a spinner host: a spinner laid out directly in it leaves a slot in `slots`.
pub fn host_spinners(cx: &mut App, host: EntityId, slots: &SpinSlots) {
    cx.default_global::<Hosts>().0.insert(host, slots.clone());
}

/// The host is gone.
pub fn drop_spinner_host(cx: &mut App, host: EntityId) {
    if let Some(hosts) = cx.try_global::<Hosts>() {
        if hosts.0.contains_key(&host) {
            cx.global_mut::<Hosts>().0.remove(&host);
        }
    }
}

/// The slots of the view being laid out, if it is a host.
pub(super) fn slots_here(window: &Window, cx: &App) -> Option<SpinSlots> {
    let hosts = cx.try_global::<Hosts>()?;
    hosts.0.get(&window.current_view()).cloned()
}

/// What a spinner in a host lays out: a box its size that notes where it ended up.
pub(super) struct Slot {
    pub slots: SpinSlots,
    pub size: Pixels,
    pub color: Color,
}

impl IntoElement for Slot {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Slot {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = self.size.into();
        style.size.height = self.size.into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        self.slots.push(SpinSlot {
            bounds,
            mask: window.content_mask().bounds,
            color: self.color,
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }
}

/// A host view in the tree (an `AnyView`, cached by the caller), noted as on screen this frame.
pub fn spin_host(slots: &SpinSlots, view: AnyView) -> impl IntoElement {
    Hosted {
        slots: slots.clone(),
        view: Some(view),
    }
}

struct Hosted {
    slots: SpinSlots,
    view: Option<AnyView>,
}

impl IntoElement for Hosted {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Hosted {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let mut element = self
            .view
            .take()
            .map(IntoElement::into_any_element)
            .unwrap_or_else(|| gpui::Empty.into_any_element());
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
        self.slots.seen();
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

/// Paints the spinners of its hosts and turns them: a view of its own, after the hosts in the
/// window's tree, so its frames draw it and its ancestors but none of the hosts.
pub struct SpinLayer {
    hosts: Vec<SpinSlots>,
}

impl SpinLayer {
    pub fn new(hosts: Vec<SpinSlots>) -> SpinLayer {
        SpinLayer { hosts }
    }
}

impl Render for SpinLayer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Painter {
            hosts: self.hosts.clone(),
            layer: cx.entity_id(),
        }
    }
}

struct Painter {
    hosts: Vec<SpinSlots>,
    layer: EntityId,
}

impl IntoElement for Painter {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// The rotation of a spinner `turns` round its box's centre, in device pixels.
fn turned(bounds: Bounds<Pixels>, turns: f32, scale: f32) -> TransformationMatrix {
    let centre = bounds.center().scale(scale);
    TransformationMatrix::unit()
        .translate(centre)
        .rotate(radians(turns * TAU))
        .translate(centre.negate())
}

impl Element for Painter {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            position: Position::Absolute,
            ..Style::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let since = spin_clock();
        let turns = turns_at(since);
        let path = Icon::Spinner.svg().unwrap_or("icons/loader-circle.svg");
        let scale = window.scale_factor();
        let mut any = false;
        for host in &self.hosts {
            for slot in host.take_shown() {
                any = true;
                let mask = ContentMask { bounds: slot.mask };
                window.with_content_mask(Some(mask), |window| {
                    window
                        .paint_svg(
                            slot.bounds,
                            path.into(),
                            turned(slot.bounds, turns, scale),
                            hsla(slot.color),
                            cx,
                        )
                        .ok();
                });
            }
        }
        if any {
            next_spin_frame(cx, self.layer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};

    fn slot(y: f32) -> SpinSlot {
        SpinSlot {
            bounds: Bounds::new(point(px(10.0), px(y)), size(px(12.0), px(12.0))),
            mask: Bounds::new(point(px(0.0), px(0.0)), size(px(200.0), px(100.0))),
            color: crate::Ink::Working.color(crate::Appearance::Dark),
        }
    }

    #[test]
    fn a_slot_scrolled_out_of_its_list_does_not_show() {
        assert!(slot(40.0).shows());
        assert!(slot(95.0).shows(), "half in still shows");
        assert!(!slot(100.0).shows());
        assert!(!slot(-30.0).shows());
    }

    #[test]
    fn a_host_shows_its_slots_only_in_a_frame_it_was_laid_out_in() {
        let slots = SpinSlots::default();
        slots.push(slot(10.0));
        assert!(slots.take_shown().is_empty(), "the host wasn't in the tree");
        slots.seen();
        assert_eq!(slots.take_shown().len(), 1);
        assert!(
            slots.take_shown().is_empty(),
            "next frame, it must be seen again"
        );
        // Drawn from its cache: seen, slots kept.
        slots.seen();
        assert_eq!(slots.take_shown().len(), 1);
        // Drawn again: its slots are laid out anew.
        slots.clear();
        slots.seen();
        assert!(slots.take_shown().is_empty());
    }

    #[test]
    fn a_spinner_turns_round_its_own_centre() {
        let b = Bounds::new(point(px(10.0), px(20.0)), size(px(12.0), px(12.0)));
        let m = turned(b, 0.25, 2.0);
        // The centre (16, 26) at 2x is (32, 52); a quarter turn leaves it where it is.
        let c = [32.0f32, 52.0];
        let x = m.rotation_scale[0][0] * c[0] + m.rotation_scale[0][1] * c[1] + m.translation[0];
        let y = m.rotation_scale[1][0] * c[0] + m.rotation_scale[1][1] * c[1] + m.translation[1];
        assert!(
            (x - 32.0).abs() < 1e-3 && (y - 52.0).abs() < 1e-3,
            "{x},{y}"
        );
        assert_eq!(turned(b, 0.0, 2.0), TransformationMatrix::unit());
    }
}
