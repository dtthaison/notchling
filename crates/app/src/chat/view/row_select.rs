//! The conversation's selection: press and drag across one message or several, double-click for a
//! word, triple-click for the paragraph, ⌘C to copy (the window's key capture asks
//! `ConversationView::copied`).
//!
//! An end of the selection names a row (the item's id, or `queued-<k>` / `sending-<k>` for the
//! messages at the tail), a block in that row (its place in `chat::blocks::plain_blocks`) and a
//! byte of the block's text. That is all the model needs: where an end sorts, and what a copy
//! takes, come from the rows and their blocks, not from what happens to be drawn, so a row that
//! isn't on screen is copied like any other. What *is* drawn only serves the pointer: each text is
//! wrapped in [`selectable`], which notes its layout as it's placed (to find the text under a
//! press), paints the selected part behind it and shows the I-beam over it. [`listener`] goes
//! first in the column: it starts each draw's notes and takes the presses, drags and releases, and
//! a change of the selection draws the conversation's own view, never the window.
//!
//! The file panel has a selection of its own (`select`): a drag doesn't run from one into the
//! other, and ⌘C copies whichever holds a selection.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    canvas, fill, AnyElement, App, Bounds, CursorStyle, DispatchPhase, Element, ElementId,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, Styled,
    TextLayout, WeakEntity, Window,
};
use notchling_engine::transcript::model::ChatItem;
use notchling_kit::ui::hsla;
use notchling_kit::{Appearance, Ink};

use super::conversation_view::ConversationView;
use super::copy_menu::selects;
use crate::chat::rows::Row;
use crate::chat::selection::{floor, ordered, range_in, word_at, At};

/// One end of a selection: a row (see [`key_of`]), a block in that row (its index in the row's
/// `plain_blocks`) and a byte offset in the block's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spot {
    pub row: SharedString,
    pub block: u16,
    pub byte: usize,
}

/// What the press took: a word or a paragraph widens the drag to whole ones of those.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    #[default]
    Char,
    Word,
    Block,
}

#[derive(Default)]
pub struct Selection {
    pub anchor: Option<Spot>,
    pub focus: Option<Spot>,
    pub unit: Unit,
    pub dragging: bool,
}

/// A text drawn this draw, for the pointer.
pub struct Note {
    row: SharedString,
    block: u16,
    text: String,
    layout: TextLayout,
}

/// Shared by the column's selectables and the listener, per `ConversationView`.
pub type SelState = Rc<RefCell<SelFrame>>;

#[derive(Default)]
pub struct SelFrame {
    pub selection: Selection,
    /// This draw's texts, in draw order, for hit tests.
    notes: Vec<Note>,
    /// Where each text's note is in `notes`.
    at: HashMap<(SharedString, u16), usize>,
    /// Row key → its place in the rows on show.
    pub rows: HashMap<SharedString, usize>,
    /// Where the pointer was at the last drag move, for the scroll that goes on while it rests
    /// past the column's edge.
    pub pointer: Option<Point<Pixels>>,
    /// A drag scroll's timer is running (only one at a time).
    pub scrolling: bool,
}

/// The row's key: the item's id; a message at the tail, which has none, is `queued-<k>` (waiting
/// for the turn to end) or `sending-<k>` (still being typed into the session), `k` its place after
/// the items, so no two tail rows share one.
pub fn key_of(items: &[ChatItem], ix: usize, row: &Row) -> SharedString {
    match items.get(ix) {
        Some(item) => SharedString::from(item.id.clone()),
        None => {
            let k = ix - items.len();
            match row {
                Row::User { sending: true, .. } => SharedString::from(format!("sending-{k}")),
                _ => SharedString::from(format!("queued-{k}")),
            }
        }
    }
}

/// Where a spot sorts: the row's place on show, then the block, with the byte in the block. `None`
/// when its row isn't on show (it was trimmed or removed).
pub fn at(spot: &Spot, rows: &HashMap<SharedString, usize>) -> Option<At> {
    let row = *rows.get(&spot.row)?;
    Some(At {
        order: order(row, spot.block),
        index: spot.byte,
    })
}

/// A block's place in the whole conversation: rows first, blocks within one.
fn order(row: usize, block: u16) -> usize {
    (row << 16) | usize::from(block)
}

impl Selection {
    /// The ends in reading order, when both rows are still on show.
    pub fn ends(&self, rows: &HashMap<SharedString, usize>) -> Option<(At, At)> {
        let anchor = at(self.anchor.as_ref()?, rows)?;
        let focus = at(self.focus.as_ref()?, rows)?;
        Some(ordered(anchor, focus))
    }

    pub fn clear(&mut self) {
        *self = Selection::default();
    }
}

/// The selected text, from the rows' blocks (`rows_in_order`: each row's key and its
/// `plain_blocks`, as on show), each block's part a line apart; `None` when nothing is selected
/// or an end's row is gone.
pub fn copied(sel: &Selection, rows_in_order: &[(SharedString, Vec<String>)]) -> Option<String> {
    let rows: HashMap<SharedString, usize> = rows_in_order
        .iter()
        .enumerate()
        .map(|(i, (key, _))| (key.clone(), i))
        .collect();
    let (start, end) = sel.ends(&rows)?;
    let mut parts: Vec<&str> = Vec::new();
    for (r, (_, blocks)) in rows_in_order
        .iter()
        .enumerate()
        .take((end.order >> 16) + 1)
        .skip(start.order >> 16)
    {
        for (b, text) in blocks.iter().enumerate() {
            let b = u16::try_from(b).unwrap_or(u16::MAX);
            let Some(range) = range_in(start, end, order(r, b), text.len()) else {
                continue;
            };
            let range = floor(text, range.start)..floor(text, range.end);
            parts.push(&text[range]);
        }
    }
    let text = parts.join("\n");
    (!text.is_empty()).then_some(text)
}

/// What ⌘C copies: the conversation's selection, else the file panel's (asked only then).
pub fn clipboard_text(
    conversation: Option<String>,
    file: impl FnOnce() -> Option<String>,
) -> Option<String> {
    conversation.or_else(file)
}

impl SelFrame {
    fn note(&self, row: &SharedString, block: u16) -> Option<&Note> {
        let i = *self.at.get(&(row.clone(), block))?;
        self.notes.get(i)
    }

    /// A text drawn this draw: its note replaces the one it has, else goes last.
    fn note_drawn(&mut self, note: Note) {
        let key = (note.row.clone(), note.block);
        match self.at.get(&key) {
            Some(&i) if i < self.notes.len() => self.notes[i] = note,
            _ => {
                self.at.insert(key, self.notes.len());
                self.notes.push(note);
            }
        }
    }

    /// The texts this draw noted: row key, block and text, in draw order (what the render check
    /// holds `plain_blocks` to).
    pub fn noted(&self) -> Vec<(SharedString, u16, String)> {
        self.notes
            .iter()
            .map(|n| (n.row.clone(), n.block, n.text.clone()))
            .collect()
    }

    /// The text under `position`: the one it's over, else the next one below it (from its start),
    /// else the last one (to its end).
    pub fn spot_at(&self, position: Point<Pixels>) -> Option<Spot> {
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
                row: note.row.clone(),
                block: note.block,
                byte: floor(&note.text, index),
            });
        }
        let last = self.notes.last()?;
        Some(Spot {
            row: last.row.clone(),
            block: last.block,
            byte: last.text.len(),
        })
    }

    /// A press on `spot` (a click, a double click, more): the selection starts as the character,
    /// word or paragraph there.
    pub fn begin(&mut self, spot: Spot, click_count: usize) {
        let text = self
            .note(&spot.row, spot.block)
            .map(|n| n.text.as_str())
            .unwrap_or_default();
        let (unit, range) = match click_count {
            1 => (Unit::Char, spot.byte..spot.byte),
            2 => (Unit::Word, word_at(text, spot.byte)),
            _ => (Unit::Block, 0..text.len()),
        };
        self.selection.unit = unit;
        self.selection.anchor = Some(Spot {
            byte: range.start,
            ..spot.clone()
        });
        self.selection.focus = Some(Spot {
            byte: range.end,
            ..spot
        });
        self.selection.dragging = true;
    }

    /// Where the drag ends with its unit: past the whole word or paragraph it's in, on the side
    /// away from the anchor.
    pub fn widen(&self, spot: Spot) -> Spot {
        let Some(note) = self.note(&spot.row, spot.block) else {
            return spot;
        };
        let forward = match (
            self.selection
                .anchor
                .as_ref()
                .and_then(|a| at(a, &self.rows)),
            at(&spot, &self.rows),
        ) {
            (Some(a), Some(f)) => f >= a,
            _ => true,
        };
        let range = match self.selection.unit {
            Unit::Char => return spot,
            Unit::Word => word_at(&note.text, spot.byte),
            Unit::Block => 0..note.text.len(),
        };
        Spot {
            byte: if forward { range.end } else { range.start },
            ..spot
        }
    }

    /// The drag reached `spot`: the focus follows (widened); whether it moved.
    pub fn drag_to(&mut self, spot: Spot) -> bool {
        let spot = self.widen(spot);
        if self.selection.focus.as_ref() == Some(&spot) {
            return false;
        }
        self.selection.focus = Some(spot);
        true
    }

    /// Drops a selection whose row is no longer on show (trimmed or removed by a reload).
    pub fn prune(&mut self) {
        let gone = |spot: &Option<Spot>| {
            spot.as_ref()
                .is_some_and(|s| !self.rows.contains_key(&s.row))
        };
        if gone(&self.selection.anchor) || gone(&self.selection.focus) {
            self.selection.clear();
        }
    }
}

/// How far a drag scrolls the conversation each tick (points; negative scrolls up) while the
/// pointer is at `pointer_y` and the column spans `top` to `bottom`: nothing inside, and beyond an
/// edge a quarter of how far past it the pointer is, between 2 and 40.
pub fn autoscroll_speed(pointer_y: f32, top: f32, bottom: f32) -> f32 {
    let past = if pointer_y < top {
        pointer_y - top
    } else if pointer_y > bottom {
        pointer_y - bottom
    } else {
        return 0.0;
    };
    (past.abs() / 4.0).clamp(2.0, 40.0).copysign(past)
}

/// Goes first in the conversation's column: starts the draw's notes, and takes the pointer.
pub fn listener(state: SelState, view: WeakEntity<ConversationView>) -> impl IntoElement {
    let begin = state.clone();
    canvas(
        move |_, _, _| {
            let mut frame = begin.borrow_mut();
            frame.notes.clear();
            frame.at.clear();
        },
        move |_, _, window, _| {
            let (down, drag, up) = (state.clone(), state.clone(), state);
            let (down_view, drag_view) = (view.clone(), view);
            window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                // With the Copy menu open a press belongs to the menu, which copies what is
                // selected and goes away on any other click: the selection stays as it is.
                let menu_open = down_view
                    .upgrade()
                    .is_some_and(|view| view.read(cx).copy_menu_open(cx));
                if menu_open && event.button == MouseButton::Left {
                    return;
                }
                let mut frame = down.borrow_mut();
                let over = frame
                    .notes
                    .iter()
                    .find(|n| n.layout.bounds().contains(&event.position))
                    .map(|n| (n.row.clone(), n.block));
                // The right button opens the Copy menu over text and leaves the selection as it is.
                if event.button == MouseButton::Right {
                    if over.is_some() {
                        let enabled = selects(
                            frame.selection.anchor.as_ref(),
                            frame.selection.focus.as_ref(),
                        );
                        drop(frame);
                        let at = event.position;
                        down_view
                            .update(cx, |view, cx| view.right_click(at, enabled, cx))
                            .ok();
                    }
                    return;
                }
                if event.button != MouseButton::Left {
                    return;
                }
                let Some((row, block)) = over else {
                    // A press anywhere else (the reply box, the sidebar) lets it go.
                    if frame.selection.anchor.take().is_some() {
                        frame.selection.focus = None;
                        drop(frame);
                        down_view.update(cx, |_, cx| cx.notify()).ok();
                    }
                    return;
                };
                let Some(spot) = frame
                    .spot_at(event.position)
                    .filter(|p| p.row == row && p.block == block)
                else {
                    return;
                };
                frame.begin(spot, event.click_count);
                drop(frame);
                down_view.update(cx, |_, cx| cx.notify()).ok();
            });
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let mut frame = drag.borrow_mut();
                if !frame.selection.dragging {
                    return;
                }
                if event.pressed_button != Some(MouseButton::Left) {
                    frame.selection.dragging = false;
                    return;
                }
                frame.pointer = Some(event.position);
                let moved = frame
                    .spot_at(event.position)
                    .is_some_and(|spot| frame.drag_to(spot));
                drop(frame);
                drag_view
                    .update(cx, |view, cx| {
                        if moved {
                            cx.notify();
                        }
                        view.drag_scroll(cx);
                    })
                    .ok();
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, _| {
                if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                    up.borrow_mut().selection.dragging = false;
                }
            });
        },
    )
    .absolute()
    .size_0()
}

/// `child` (a text whose layout is `layout`, of `text`) made selectable as block `block` of row
/// `row`.
pub fn selectable(
    row: SharedString,
    block: u16,
    text: String,
    layout: TextLayout,
    child: AnyElement,
    a: Appearance,
    state: &SelState,
) -> Selectable {
    Selectable {
        row,
        block,
        text,
        layout,
        child: Some(child),
        a,
        state: state.clone(),
    }
}

pub struct Selectable {
    row: SharedString,
    block: u16,
    text: String,
    layout: TextLayout,
    child: Option<AnyElement>,
    a: Appearance,
    state: SelState,
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
        self.state.borrow_mut().note_drawn(Note {
            row: self.row.clone(),
            block: self.block,
            text: std::mem::take(&mut self.text),
            layout: self.layout.clone(),
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
        let selected = {
            let frame = self.state.borrow();
            frame.selection.ends(&frame.rows).and_then(|(start, end)| {
                let row = *frame.rows.get(&self.row)?;
                let len = frame.note(&self.row, self.block)?.text.len();
                range_in(start, end, order(row, self.block), len)
            })
        };
        // Over the text, not behind it: inline code paints its own background, which hid a wash
        // drawn first.
        child.paint(window, cx);
        if let Some(range) = selected {
            let wash = hsla(Ink::Accent.color(self.a).with_alpha(0.32));
            for rect in super::select::rects(&self.layout, range) {
                window.paint_quad(fill(rect, wash));
            }
        }
    }
}

#[cfg(test)]
#[path = "row_select_tests.rs"]
mod tests;
