//! Selecting a conversation's text with the pointer, since GPUI 0.2.2's text can't be selected:
//! press and drag across one message or several, double-click for a word, triple-click for the
//! paragraph, ⌘C to copy (`ChatView`'s key capture asks [`copied`]).
//!
//! Each text drawn in the conversation is wrapped in [`selectable`], which notes its layout as it's
//! placed (in reading order, so the order of the notes is the order of the text), paints the
//! selected part behind it and shows the I-beam over it. [`listener`] goes first in the column: it
//! starts each frame's notes and takes the presses, drags and releases. The positions themselves
//! are `chat::selection`'s.

use std::cell::RefCell;
use std::ops::Range;

use gpui::{
    canvas, fill, point, px, size, AnyElement, App, Bounds, CursorStyle, DispatchPhase, Element,
    ElementId, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Styled, TextLayout,
    Window,
};
use notchling_kit::ui::hsla;
use notchling_kit::{Appearance, Ink};

use crate::chat::selection::{copied as copy_of, floor, ordered, range_in, word_at, At};

/// A text drawn this frame: its key (stable across frames), its text and where it was laid out.
struct Note {
    key: String,
    text: String,
    layout: TextLayout,
}

/// An end of the selection by the text's key, so it survives the frames in between.
#[derive(Clone, PartialEq, Eq)]
struct Spot {
    key: String,
    index: usize,
}

#[derive(Default)]
struct State {
    notes: Vec<Note>,
    anchor: Option<Spot>,
    focus: Option<Spot>,
    /// What the press took: a word or a paragraph widens the drag to whole ones of those.
    unit: Unit,
    dragging: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Unit {
    #[default]
    Char,
    Word,
    Block,
}

thread_local! {
    /// One pointer and one Chat window, so one selection.
    static STATE: RefCell<State> = RefCell::new(State::default());
}

impl State {
    fn order(&self, spot: &Spot) -> Option<At> {
        let order = self.notes.iter().position(|n| n.key == spot.key)?;
        Some(At {
            order,
            index: spot.index,
        })
    }

    /// The selection's ends in reading order, when both texts are still drawn.
    fn ends(&self) -> Option<(At, At)> {
        let anchor = self.order(self.anchor.as_ref()?)?;
        let focus = self.order(self.focus.as_ref()?)?;
        Some(ordered(anchor, focus))
    }

    /// The text under `position`: the one it's over, else the next one below it (from its start),
    /// else the last one (to its end).
    fn spot_at(&self, position: Point<Pixels>) -> Option<Spot> {
        for note in &self.notes {
            let bounds = note.layout.bounds();
            if position.y > bounds.bottom() {
                continue;
            }
            let index = if position.y < bounds.top() {
                0
            } else {
                note.layout
                    .index_for_position(position)
                    .unwrap_or_else(|nearest| nearest)
            };
            return Some(Spot {
                key: note.key.clone(),
                index: floor(&note.text, index),
            });
        }
        let last = self.notes.last()?;
        Some(Spot {
            key: last.key.clone(),
            index: last.text.len(),
        })
    }

    fn note(&self, key: &str) -> Option<&Note> {
        self.notes.iter().find(|n| n.key == key)
    }

    /// Where the drag ends with `unit`: past the whole word or paragraph it's in, on the side
    /// away from the anchor.
    fn widen(&self, spot: Spot) -> Spot {
        let Some(note) = self.note(&spot.key) else {
            return spot;
        };
        let forward = match (
            self.anchor.as_ref().and_then(|a| self.order(a)),
            self.order(&spot),
        ) {
            (Some(a), Some(f)) => f >= a,
            _ => true,
        };
        let range = match self.unit {
            Unit::Char => return spot,
            Unit::Word => word_at(&note.text, spot.index),
            Unit::Block => 0..note.text.len(),
        };
        Spot {
            index: if forward { range.end } else { range.start },
            ..spot
        }
    }
}

/// The selected text, for ⌘C; `None` when nothing is selected.
pub fn copied() -> Option<String> {
    STATE.with(|s| {
        let s = s.borrow();
        let (start, end) = s.ends()?;
        let blocks: Vec<&str> = s.notes.iter().map(|n| n.text.as_str()).collect();
        let text = copy_of(&blocks, start, end);
        (!text.is_empty()).then_some(text)
    })
}

/// Forgets the selection (another session opened).
pub fn clear() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.anchor = None;
        s.focus = None;
        s.dragging = false;
    });
}

/// Goes first in the conversation's column: starts the frame's notes, and takes the pointer.
pub fn listener() -> impl IntoElement {
    canvas(
        |_, _, _| STATE.with(|s| s.borrow_mut().notes.clear()),
        |_, _, window, _| {
            window.on_mouse_event(|event: &MouseDownEvent, phase, window, _| {
                if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                    return;
                }
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    let over = s
                        .notes
                        .iter()
                        .find(|n| n.layout.bounds().contains(&event.position))
                        .map(|n| n.key.clone());
                    let Some(key) = over else {
                        // A press anywhere else (the reply box, the sidebar) lets it go.
                        if s.anchor.take().is_some() {
                            s.focus = None;
                            window.refresh();
                        }
                        return;
                    };
                    let Some(spot) = s.spot_at(event.position).filter(|p| p.key == key) else {
                        return;
                    };
                    let note = s.note(&key).map(|n| n.text.clone()).unwrap_or_default();
                    let (unit, range) = match event.click_count {
                        1 => (Unit::Char, spot.index..spot.index),
                        2 => (Unit::Word, word_at(&note, spot.index)),
                        _ => (Unit::Block, 0..note.len()),
                    };
                    s.unit = unit;
                    s.anchor = Some(Spot {
                        key: key.clone(),
                        index: range.start,
                    });
                    s.focus = Some(Spot {
                        key,
                        index: range.end,
                    });
                    s.dragging = true;
                    window.refresh();
                });
            });
            window.on_mouse_event(|event: &MouseMoveEvent, phase, window, _| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    if !s.dragging {
                        return;
                    }
                    if event.pressed_button != Some(MouseButton::Left) {
                        s.dragging = false;
                        return;
                    }
                    let Some(spot) = s.spot_at(event.position) else {
                        return;
                    };
                    let spot = s.widen(spot);
                    if s.focus.as_ref() != Some(&spot) {
                        s.focus = Some(spot);
                        window.refresh();
                    }
                });
            });
            window.on_mouse_event(|event: &MouseUpEvent, phase, _, _| {
                if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                    STATE.with(|s| s.borrow_mut().dragging = false);
                }
            });
        },
    )
    .absolute()
    .size_0()
}

/// `child` (a text whose layout is `layout`, of `text`) made selectable under `key`, which must be
/// unique in the conversation and the same each frame.
pub fn selectable(
    key: impl Into<String>,
    text: impl Into<String>,
    layout: TextLayout,
    child: AnyElement,
    a: Appearance,
) -> Selectable {
    Selectable {
        key: key.into(),
        text: text.into(),
        layout,
        child: Some(child),
        a,
    }
}

pub struct Selectable {
    key: String,
    text: String,
    layout: TextLayout,
    child: Option<AnyElement>,
    a: Appearance,
}

impl IntoElement for Selectable {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Selectable {
    type RequestLayoutState = AnyElement;
    type PrepaintState = Hitbox;

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
        let mut child = self
            .child
            .take()
            .unwrap_or_else(|| gpui::Empty.into_any_element());
        (child.request_layout(window, cx), child)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        child.prepaint(window, cx);
        STATE.with(|s| {
            s.borrow_mut().notes.push(Note {
                key: self.key.clone(),
                text: std::mem::take(&mut self.text),
                layout: self.layout.clone(),
            })
        });
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        child: &mut AnyElement,
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.set_cursor_style(CursorStyle::IBeam, hitbox);
        let selected = STATE.with(|s| {
            let s = s.borrow();
            let (start, end) = s.ends()?;
            let order = s.notes.iter().position(|n| n.key == self.key)?;
            let len = s.notes[order].text.len();
            range_in(start, end, order, len)
        });
        if let Some(range) = selected {
            let wash = hsla(Ink::Accent.color(self.a).with_alpha(0.32));
            for rect in rects(&self.layout, range) {
                window.paint_quad(fill(rect, wash));
            }
        }
        child.paint(window, cx);
    }
}

/// The rectangles behind `range` of a laid-out text: one per line it covers, found by asking where
/// each character starts (a line ends where the next character jumps down a line).
fn rects(layout: &TextLayout, range: Range<usize>) -> Vec<Bounds<Pixels>> {
    let height = layout.line_height();
    let text = layout.text();
    let mut rows: Vec<(Pixels, Pixels, Pixels)> = Vec::new(); // (top, left, right)
    let mut push = |top: Pixels, left: Pixels, right: Pixels| match rows.last_mut() {
        Some(row) if row.0 == top => {
            row.1 = row.1.min(left);
            row.2 = row.2.max(right);
        }
        _ => rows.push((top, left, right)),
    };
    let mut index = floor(&text, range.start);
    while index < range.end.min(text.len()) {
        let next = text[index..]
            .chars()
            .next()
            .map_or(index + 1, |c| index + c.len_utf8());
        let (Some(here), there) = (
            layout.position_for_index(index),
            layout.position_for_index(next),
        ) else {
            break;
        };
        // The last character of a line: its width from the line height's share, as the next
        // character's start is on the line below.
        let right = match there {
            Some(p) if p.y == here.y => p.x,
            _ => here.x + height * 0.3,
        };
        push(here.y, here.x, right);
        index = next;
    }
    rows.into_iter()
        .map(|(top, left, right)| Bounds {
            origin: point(left, top),
            size: size((right - left).max(px(2.0)), height),
        })
        .collect()
}
