//! A text editor over GPUI's platform input handler, for the reply box and any field that needs a
//! caret. Typing arrives through `NSTextInputClient` (`replace_text_in_range`), so input methods
//! compose with marked text (Vietnamese Telex, Japanese), drawn underlined, and the candidate
//! window sits under the composed word (`bounds_for_range`). Click places the caret, drag selects,
//! a double click picks a word and a triple one a line; the keys are a Mac text view's: ⌥ and ⌘
//! with the arrows and deletes, ⌘A, ⌘C/⌘X/⌘V (plain text), ⌘Z and ⌘⇧Z. The text wraps at the
//! box's width; the box grows from one line to `max_lines`, then scrolls, keeping the caret in
//! view. Return submits and Shift or Option Return adds a line ([`crate::text_edit::return_key`]).
//! Esc, Tab and keys it doesn't know pass on to the views around it.
//!
//! The model is [`TextEdit`]; this file is the entity (focus, keys, events, the caret's blink),
//! `element` lays the text out and paints it, `input` is the input handler and the mouse.

mod element;
mod input;

use std::ops::Range;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable,
    KeyDownEvent, Modifiers, Pixels, Render, ScrollWheelEvent, SharedString, Window,
};

use crate::text_edit::{return_key, ReturnKey, TextEdit, Unit};
use element::{Layout, TextElement};

/// What the editor tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEditorEvent {
    /// The text is different.
    Changed,
    /// Return (or ⌘ or Ctrl Return): the owner takes the text.
    Submit,
}

/// The caret blinks at this pace while the editor is focused, after a beat with it on.
const BLINK: Duration = Duration::from_millis(500);

/// A mouse drag in progress: what the first click picked (a point, a word or a line), which the
/// selection always keeps.
struct Drag {
    anchor: Range<usize>,
    by: Unit,
}

pub struct TextEditor {
    edit: TextEdit,
    focus: FocusHandle,
    placeholder: SharedString,
    enabled: bool,
    /// The box grows to this many lines, then scrolls; `0` for no limit.
    max_lines: usize,
    /// Return with Shift or Option adds a line. A one-line field submits on any Return.
    multiline: bool,
    /// Draws the caret and selection without focus and without blinking, for a gallery render.
    preview: bool,
    /// The last layout, for the mouse, the input method and vertical moves.
    layout: Option<Layout>,
    scroll_top: Pixels,
    /// Bring the caret into view at the next layout (after an edit or a move, not a wheel scroll).
    reveal_caret: bool,
    /// The x the caret keeps while moving up and down through rows of different lengths.
    goal_x: Option<Pixels>,
    drag: Option<Drag>,
    caret_on: bool,
    /// Something just happened: the caret stays on through the next blink.
    hold_caret: bool,
    /// Whether the last render saw the editor focused (the blink loop has no window).
    focused: bool,
}

impl EventEmitter<TextEditorEvent> for TextEditor {}

impl Focusable for TextEditor {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextEditor {
    /// An empty, enabled editor that grows to `max_lines` (0 for no limit).
    pub fn new(placeholder: &str, max_lines: usize, cx: &mut Context<Self>) -> TextEditor {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(BLINK).await;
            if this.update(cx, |editor, cx| editor.blink(cx)).is_err() {
                break;
            }
        })
        .detach();
        TextEditor {
            edit: TextEdit::default(),
            focus: cx.focus_handle(),
            placeholder: SharedString::from(placeholder.to_owned()),
            enabled: true,
            max_lines,
            multiline: true,
            preview: false,
            layout: None,
            scroll_top: px(0.0),
            reveal_caret: true,
            goal_x: None,
            drag: None,
            caret_on: true,
            hold_caret: false,
            focused: false,
        }
    }

    /// A one-line field: Return always submits, and nothing wraps or grows.
    pub fn single_line(mut self) -> TextEditor {
        self.multiline = false;
        self.max_lines = 1;
        self
    }

    /// For a render: the caret and selection show without focus, steady.
    pub fn preview(mut self) -> TextEditor {
        self.preview = true;
        self
    }

    pub fn text(&self) -> &str {
        self.edit.text()
    }

    pub fn is_empty(&self) -> bool {
        self.edit.is_empty()
    }

    /// The model, for what the editor shows (selection, marked text).
    pub fn edit(&self) -> &TextEdit {
        &self.edit
    }

    /// Another text (another session's draft): the caret at its end, no history, no event.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.edit.text() == text {
            return;
        }
        self.edit.set_text(text);
        self.scroll_top = px(0.0);
        self.touched(false, cx);
    }

    pub fn set_placeholder(&mut self, placeholder: &str, cx: &mut Context<Self>) {
        if self.placeholder.as_ref() != placeholder {
            self.placeholder = SharedString::from(placeholder.to_owned());
            cx.notify();
        }
    }

    /// Off: no focus, no keys, no caret, dimmed. The text stays.
    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.drag = None;
            cx.notify();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Selects from `anchor` to `head` (for a render, or an owner placing the caret).
    pub fn select(&mut self, anchor: usize, head: usize, cx: &mut Context<Self>) {
        self.edit.set_selection(anchor, head);
        self.touched(false, cx);
    }

    /// Marks `range` as composing (for a render: the input method does this live).
    pub fn mark(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let range = self.edit.snap(range.start)..self.edit.snap(range.end);
        let (text, utf16) = (
            self.edit.text().to_owned(),
            self.edit.utf8_to_utf16(range.start),
        );
        let marked = text[range.start..range.end.max(range.start)].to_owned();
        let end = utf16 + marked.encode_utf16().count();
        self.edit
            .replace_and_mark(Some(utf16..end), &marked, Some(end - utf16..end - utf16));
        cx.notify();
    }

    /// Inserts `text` at the caret as if pasted: its own undo step.
    pub fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        self.edit.paste(text);
        self.touched(true, cx);
    }

    /// An edit made on a copy of the model (`edit().clone()`, then the owner's pure function):
    /// takes it as the text, telling the owner when the text differs.
    pub fn set_edit(&mut self, edit: TextEdit, cx: &mut Context<Self>) {
        let changed = edit.text() != self.edit.text();
        self.edit = edit;
        self.touched(changed, cx);
    }

    pub fn focus(&self, window: &mut Window) {
        if self.enabled {
            window.focus(&self.focus);
        }
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.enabled && self.focus.is_focused(window)
    }

    /// After anything the user did: the caret shows and comes into view; `changed` tells the owner.
    fn touched(&mut self, changed: bool, cx: &mut Context<Self>) {
        self.reveal_caret = true;
        self.caret_on = true;
        self.hold_caret = true;
        if changed {
            cx.emit(TextEditorEvent::Changed);
        }
        cx.notify();
    }

    fn blink(&mut self, cx: &mut Context<Self>) {
        if !self.focused || self.preview {
            if !self.caret_on {
                self.caret_on = true;
                cx.notify();
            }
            return;
        }
        if std::mem::take(&mut self.hold_caret) {
            return;
        }
        self.caret_on = !self.caret_on;
        cx.notify();
    }

    /// ⌘ reaches the line, ⌥ the word, else one character.
    fn unit(m: &Modifiers) -> Unit {
        if m.platform {
            Unit::Line
        } else if m.alt {
            Unit::Word
        } else {
            Unit::Grapheme
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        let k = &event.keystroke;
        let m = &k.modifiers;
        let (cmd, extend) = (m.platform, m.shift);
        let before = self.edit.text().to_owned();
        let mut vertical = false;
        match k.key.as_str() {
            "enter" => {
                let key = if cmd || m.control || !self.multiline {
                    ReturnKey::Submit
                } else {
                    return_key(m.shift, m.alt)
                };
                match key {
                    ReturnKey::Newline => self.edit.insert("\n"),
                    ReturnKey::Submit => cx.emit(TextEditorEvent::Submit),
                }
            }
            "backspace" => self.edit.delete_backward(Self::unit(m)),
            "delete" => self.edit.delete_forward(Self::unit(m)),
            "left" if cmd => self.row_edge(false, extend),
            "right" if cmd => self.row_edge(true, extend),
            "left" => self.edit.move_left(Self::unit(m), extend),
            "right" => self.edit.move_right(Self::unit(m), extend),
            "up" if cmd => self.edit.move_left(Unit::Document, extend),
            "down" if cmd => self.edit.move_right(Unit::Document, extend),
            "up" => {
                vertical = true;
                self.vertical(-1, extend);
            }
            "down" => {
                vertical = true;
                self.vertical(1, extend);
            }
            "home" => self.edit.move_left(Unit::Document, extend),
            "end" => self.edit.move_right(Unit::Document, extend),
            "a" if cmd && !m.shift && !m.alt => self.edit.select_all(),
            "c" if cmd => {
                if !self.edit.selection().is_empty() {
                    let text = self.edit.selected_text().to_owned();
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            "x" if cmd => {
                if let Some(text) = self.edit.cut() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            "v" if cmd => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let text = if self.multiline {
                        text
                    } else {
                        text.replace(['\n', '\r'], " ")
                    };
                    self.edit.paste(&text);
                }
            }
            "z" | "Z" if cmd => {
                if m.shift {
                    self.edit.redo();
                } else {
                    self.edit.undo();
                }
            }
            // Esc, Tab, typed characters (they come through the input handler) and the rest go on.
            _ => return,
        }
        if !vertical {
            self.goal_x = None;
        }
        self.touched(before != self.edit.text(), cx);
        cx.stop_propagation();
    }

    /// Up or Down: the caret keeps its x through rows of different lengths; past the first or
    /// last row it goes to the text's start or end, as a Mac text view's does.
    fn vertical(&mut self, direction: i32, extend: bool) {
        let Some(layout) = &self.layout else {
            return self.edit.move_left(Unit::Document, extend);
        };
        let caret = layout.position_for_index(self.edit.caret());
        let x = *self.goal_x.get_or_insert(caret.x);
        let y = caret.y + layout.line_height * (direction as f32) + layout.line_height / 2.0;
        let target = if y < px(0.0) {
            0
        } else if y > layout.content_height() {
            self.edit.text().len()
        } else {
            layout.index_for_point(gpui::point(x, y))
        };
        if extend {
            self.edit.select_to(target);
        } else {
            self.edit.move_to(target);
        }
    }

    /// ⌘← and ⌘→: the edge of the wrapped row the caret is on (the paragraph's, without a
    /// layout).
    fn row_edge(&mut self, end: bool, extend: bool) {
        let target = match &self.layout {
            Some(layout) => {
                let row = layout.row_at(self.edit.caret());
                if end {
                    row.end
                } else {
                    row.start
                }
            }
            None => {
                let range = self.edit.line_range_at(self.edit.caret());
                if end {
                    range.end
                } else {
                    range.start
                }
            }
        };
        if extend {
            self.edit.select_to(target);
        } else {
            self.edit.move_to(target);
        }
    }

    /// The wheel scrolls the text once it is taller than the box; before that the event goes on
    /// to whatever scrolls behind.
    fn wheel(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = &self.layout else {
            return;
        };
        let max = layout.max_scroll();
        if max <= px(0.0) {
            return;
        }
        let delta = event.delta.pixel_delta(layout.line_height).y;
        let top = (self.scroll_top - delta).clamp(px(0.0), max);
        if top != self.scroll_top {
            self.scroll_top = top;
            cx.notify();
        }
        cx.stop_propagation();
    }
}

impl Render for TextEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.enabled && self.focus.is_focused(window) {
            window.blur();
        }
        let focused = self.is_focused(window);
        if focused != self.focused {
            self.focused = focused;
            self.caret_on = true;
        }
        let element = TextElement::new(cx.entity());
        let root = div().w_full().min_w(px(0.0)).child(element);
        if self.enabled {
            root.id("text-editor")
                .track_focus(&self.focus)
                .cursor_text()
                .on_key_down(cx.listener(Self::key_down))
                .on_scroll_wheel(cx.listener(Self::wheel))
                .into_any_element()
        } else {
            root.opacity(0.5).into_any_element()
        }
    }
}
