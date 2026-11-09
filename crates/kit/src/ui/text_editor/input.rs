//! The platform's side of the editor: `NSTextInputClient` through GPUI's [`EntityInputHandler`]
//! (typed and composed text, the input method's marked range and candidate window position), in
//! UTF-16 units, and the mouse (place, drag, double and triple click).

use std::ops::Range;

use gpui::{
    point, Bounds, Context, EntityInputHandler, MouseDownEvent, MouseMoveEvent, Pixels, Point,
    UTF16Selection, Window,
};

use super::{Drag, TextEditor};
use crate::text_edit::Unit;

impl TextEditor {
    /// The text index under a window point, from the last layout.
    pub(super) fn index_at(&self, position: Point<Pixels>) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let local = point(
            position.x - layout.bounds.left(),
            position.y - layout.bounds.top() + layout.scroll_top,
        );
        Some(layout.index_for_point(local))
    }

    pub(super) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }
        let Some(index) = self.index_at(event.position) else {
            return;
        };
        self.focus(window);
        let anchor = match event.click_count {
            0 | 1 => {
                if event.modifiers.shift {
                    self.edit.select_to(index);
                } else {
                    self.edit.move_to(index);
                }
                index..index
            }
            2 => {
                let word = self.edit.word_range_at(index);
                self.edit.set_selection(word.start, word.end);
                word
            }
            _ => {
                let line = self.edit.line_range_at(index);
                self.edit.set_selection(line.start, line.end);
                line
            }
        };
        let by = match event.click_count {
            0 | 1 => Unit::Grapheme,
            2 => Unit::Word,
            _ => Unit::Line,
        };
        self.drag = Some(Drag { anchor, by });
        self.goal_x = None;
        self.touched(false, cx);
    }

    /// Dragging extends from what the first click picked, by the same unit, and scrolls the box
    /// along with the caret.
    pub(super) fn mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = &self.drag else {
            return;
        };
        let Some(index) = self.index_at(event.position) else {
            return;
        };
        let target = match drag.by {
            Unit::Word => self.edit.word_range_at(index),
            Unit::Line => self.edit.line_range_at(index),
            Unit::Grapheme | Unit::Document => index..index,
        };
        let anchor = drag.anchor.clone();
        if target.start < anchor.start {
            self.edit.set_selection(anchor.end, target.start);
        } else if target.end > anchor.end {
            self.edit.set_selection(anchor.start, target.end);
        } else {
            self.edit.set_selection(anchor.start, anchor.end);
        }
        self.touched(false, cx);
    }

    pub(super) fn mouse_up(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            cx.notify();
        }
    }
}

impl EntityInputHandler for TextEditor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let (text, actual) = self.edit.text_for_range_utf16(range_utf16);
        *adjusted_range = Some(actual);
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let selection = self.edit.selection_utf16();
        Some(UTF16Selection {
            range: selection.range,
            reversed: selection.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.edit.marked_utf16()
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.edit.unmark();
        cx.notify();
    }

    /// Typed text, a committed composition, or a held key's accent (`insertText`).
    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }
        let text = if self.multiline {
            text.to_owned()
        } else {
            text.replace(['\n', '\r'], " ")
        };
        self.edit.replace_text(range_utf16, &text);
        self.goal_x = None;
        self.touched(true, cx);
    }

    /// The composition so far (`setMarkedText`), underlined until it is committed.
    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }
        self.edit
            .replace_and_mark(range_utf16, new_text, new_selected_range_utf16);
        self.goal_x = None;
        self.touched(true, cx);
    }

    /// Where a range is on screen: the input method puts its candidate window under it. A range
    /// across rows reports its first row to the right edge.
    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let start = layout.position_for_index(self.edit.utf16_to_utf8(range_utf16.start));
        let end = layout.position_for_index(self.edit.utf16_to_utf8(range_utf16.end));
        let right = if end.y == start.y {
            end.x
        } else {
            element_bounds.size.width
        };
        let top = element_bounds.top() + start.y - layout.scroll_top;
        Some(Bounds::from_corners(
            point(element_bounds.left() + start.x, top),
            point(element_bounds.left() + right, top + layout.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let index = self.index_at(position)?;
        Some(self.edit.utf8_to_utf16(index))
    }
}
