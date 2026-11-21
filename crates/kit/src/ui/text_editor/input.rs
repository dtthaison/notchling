use std::ops::Range;

use gpui::{
    point, Bounds, Context, EntityInputHandler, MouseDownEvent, MouseMoveEvent, Pixels, Point,
    UTF16Selection, Window,
};

use super::TextEditor;

impl TextEditor {
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
        let press = if self.masked {
            self.edit
                .press_at_masked(index, event.click_count, event.modifiers.shift)
        } else {
            self.edit
                .press_at(index, event.click_count, event.modifiers.shift)
        };
        self.drag = Some(press);
        self.goal_x = None;
        self.touched(false, cx);
    }

    pub(super) fn mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(press) = self.drag.clone() else {
            return;
        };
        let Some(index) = self.index_at(event.position) else {
            return;
        };
        self.edit.drag_to(&press, index);
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
            super::single_line_text(text, self.masked)
        };
        self.edit.replace_text(range_utf16, &text);
        self.goal_x = None;
        self.touched(true, cx);
    }

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
