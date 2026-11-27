use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::TAU;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use gpui::{
    prelude::*, px, radians, AnyElement, AnyView, App, Bounds, ContentMask, Context, Element,
    ElementId, EntityId, Global, GlobalElementId, InspectorElementId, LayoutId, Negate, Pixels,
    Position, Style, TransformationMatrix, Window,
};

use super::hsla;
use super::icons::{next_spin_frame, spin_clock, turns_at};
use super::spin_native::{mix_key, NativeSpin, SpinGate, SpinSink};
use crate::{Color, Icon};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpinSlot {
    pub key: u64,
    pub bounds: Bounds<Pixels>,
    pub mask: Bounds<Pixels>,
    pub color: Color,
}

impl SpinSlot {
    pub fn shows(&self) -> bool {
        let shown = self.bounds.intersect(&self.mask);
        shown.size.width > px(0.0) && shown.size.height > px(0.0)
    }
}

#[derive(Default)]
struct Slots {
    only: Option<&'static [&'static str]>,
    slots: Vec<SpinSlot>,
    on_screen: bool,
}

#[derive(Clone, Default)]
pub struct SpinSlots(Rc<RefCell<Slots>>);

impl SpinSlots {
    pub fn only(names: &'static [&'static str]) -> SpinSlots {
        SpinSlots(Rc::new(RefCell::new(Slots {
            only: Some(names),
            ..Slots::default()
        })))
    }

    fn takes(&self, id: &ElementId) -> bool {
        match (self.0.borrow().only, id) {
            (None, _) => true,
            (Some(names), ElementId::Name(name)) => names.contains(&name.as_ref()),
            (Some(_), _) => false,
        }
    }

    pub fn clear(&self) {
        self.0.borrow_mut().slots.clear();
    }

    pub fn push(&self, slot: SpinSlot) {
        self.0.borrow_mut().slots.push(slot);
    }

    pub fn len(&self) -> usize {
        self.0.borrow().slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn take_shown(&self) -> Vec<SpinSlot> {
        let mut s = self.0.borrow_mut();
        let on_screen = std::mem::take(&mut s.on_screen);
        if on_screen {
            s.slots.iter().copied().filter(SpinSlot::shows).collect()
        } else {
            Vec::new()
        }
    }

    pub fn seen(&self) {
        self.0.borrow_mut().on_screen = true;
    }
}

#[derive(Default)]
struct Hosts(HashMap<EntityId, SpinSlots>);

impl Global for Hosts {}

pub fn host_spinners(cx: &mut App, host: EntityId, slots: &SpinSlots) {
    cx.default_global::<Hosts>().0.insert(host, slots.clone());
}

pub fn drop_spinner_host(cx: &mut App, host: EntityId) {
    if let Some(hosts) = cx.try_global::<Hosts>() {
        if hosts.0.contains_key(&host) {
            cx.global_mut::<Hosts>().0.remove(&host);
        }
    }
}

pub(super) fn slots_here(window: &Window, cx: &App, id: &ElementId) -> Option<SpinSlots> {
    let hosts = cx.try_global::<Hosts>()?;
    hosts
        .0
        .get(&window.current_view())
        .filter(|slots| slots.takes(id))
        .cloned()
}

pub(super) fn key_of(id: Option<&GlobalElementId>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

pub(super) struct Slot {
    pub key: u64,
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
            key: self.key,
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

pub struct SpinLayer {
    hosts: Vec<SpinSlots>,
    native: Option<Rc<NativeSpin>>,
}

impl SpinLayer {
    pub fn new(hosts: Vec<SpinSlots>) -> SpinLayer {
        SpinLayer {
            hosts,
            native: None,
        }
    }

    pub fn native(hosts: Vec<SpinSlots>, sink: Rc<dyn SpinSink>, allowed: SpinGate) -> SpinLayer {
        SpinLayer {
            hosts,
            native: Some(Rc::new(NativeSpin::new(sink, allowed))),
        }
    }
}

impl Render for SpinLayer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Painter {
            hosts: self.hosts.clone(),
            layer: cx.entity_id(),
            native: self.native.clone(),
        }
    }
}

struct Painter {
    hosts: Vec<SpinSlots>,
    layer: EntityId,
    native: Option<Rc<NativeSpin>>,
}

impl IntoElement for Painter {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

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
        if let Some(native) = &self.native {
            if native.allowed() {
                let slots: Vec<SpinSlot> = self
                    .hosts
                    .iter()
                    .enumerate()
                    .flat_map(|(place, host)| {
                        host.take_shown().into_iter().map(move |slot| SpinSlot {
                            key: mix_key(place, slot.key),
                            ..slot
                        })
                    })
                    .collect();
                native.sync(window, &slots);
                return;
            }
            native.leave(window);
        }
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
            key: 1,
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
        slots.seen();
        assert_eq!(slots.take_shown().len(), 1);
        slots.clear();
        slots.seen();
        assert!(slots.take_shown().is_empty());
    }

    #[test]
    fn a_spinner_turns_round_its_own_centre() {
        let b = Bounds::new(point(px(10.0), px(20.0)), size(px(12.0), px(12.0)));
        let m = turned(b, 0.25, 2.0);
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
