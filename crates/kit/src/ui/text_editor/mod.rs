pub mod actions;
mod element;
mod input;

use std::ops::Range;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable,
    KeyDownEvent, Modifiers, Pixels, Render, ScrollWheelEvent, SharedString, Window,
};

use crate::text_edit::{return_key, Press, ReturnKey, TextEdit, Unit};
use element::{Layout, TextElement};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEditorEvent {
    Changed,
    Submit,
}

const BLINK: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Cut,
    Copy,
    Paste,
    SelectAll,
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    Run,
    Swallow,
    Pass,
}

pub(crate) fn route(command: Command, selection_empty: bool, blocked: bool) -> Route {
    if !matches!(command, Command::Copy | Command::Cut) {
        Route::Run
    } else if blocked {
        Route::Swallow
    } else if selection_empty {
        Route::Pass
    } else {
        Route::Run
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Veil {
    secret: bool,
    shown: bool,
}

impl Veil {
    pub(crate) fn hidden_secret() -> Veil {
        Veil {
            secret: true,
            shown: false,
        }
    }

    pub(crate) fn set(self, masked: bool) -> Veil {
        Veil {
            secret: self.secret || masked,
            shown: self.secret && !masked,
        }
    }

    pub(crate) fn masks(self) -> bool {
        self.secret && !self.shown
    }

    pub(crate) fn is_secret(self) -> bool {
        self.secret
    }

    pub(crate) fn single_line(self, text: &str) -> String {
        single_line_text(text, self.secret)
    }

    pub(crate) fn input_range(
        self,
        asked: Range<usize>,
        marked: Option<Range<usize>>,
    ) -> Option<Range<usize>> {
        let composing = marked.is_some_and(|m| m.start <= asked.start && asked.end <= m.end);
        (!self.secret || composing).then_some(asked)
    }
}

pub(crate) fn is_shortcut(m: &Modifiers) -> bool {
    m.platform && !m.alt && !m.control
}

pub(crate) fn edit_shortcut(m: &Modifiers) -> bool {
    is_shortcut(m) && !m.shift
}

pub(crate) fn shortcut_letter(key: &str) -> String {
    key.to_ascii_lowercase()
}

pub fn single_line_text(text: &str, masked: bool) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\n' | '\r' if masked => None,
            '\n' | '\r' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

pub struct TextEditor {
    edit: TextEdit,
    focus: FocusHandle,
    placeholder: SharedString,
    enabled: bool,
    max_lines: usize,
    multiline: bool,
    grows: bool,
    preview: bool,
    veil: Veil,
    arrows_pass: bool,
    layout: Option<Layout>,
    scroll_top: Pixels,
    reveal_caret: bool,
    goal_x: Option<Pixels>,
    drag: Option<Press>,
    caret_on: bool,
    hold_caret: bool,
    focused: bool,
}

#[cfg(test)]
mod command_tests;

impl EventEmitter<TextEditorEvent> for TextEditor {}

impl Focusable for TextEditor {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextEditor {
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
            grows: false,
            preview: false,
            veil: Veil::default(),
            arrows_pass: false,
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

    pub fn single_line(mut self) -> TextEditor {
        self.multiline = false;
        self.max_lines = 1;
        self
    }

    pub fn grows_with_lines(mut self) -> TextEditor {
        self.grows = true;
        self
    }

    pub(crate) fn rows_allowed(&self) -> usize {
        if self.grows && !self.edit.text().contains(['\n', '\r']) {
            1
        } else {
            self.max_lines
        }
    }

    pub fn masked(mut self) -> TextEditor {
        self.veil = Veil::hidden_secret();
        self.edit.stop_history();
        self
    }

    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        let veil = self.veil.set(masked);
        if veil == self.veil {
            return;
        }
        self.veil = veil;
        self.edit.stop_history();
        self.layout = None;
        cx.notify();
    }

    pub fn arrows_pass(mut self) -> TextEditor {
        self.arrows_pass = true;
        self
    }

    pub fn is_masked(&self) -> bool {
        self.veil.masks()
    }

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

    pub fn edit(&self) -> &TextEdit {
        &self.edit
    }

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

    pub fn select(&mut self, anchor: usize, head: usize, cx: &mut Context<Self>) {
        self.edit.set_selection(anchor, head);
        self.touched(false, cx);
    }

    pub fn preview_selection(&mut self, anchor: usize, head: usize, cx: &mut Context<Self>) {
        self.preview = true;
        self.select(anchor, head, cx);
    }

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

    pub fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        self.edit.paste(text);
        self.touched(true, cx);
    }

    pub fn set_edit(&mut self, mut edit: TextEdit, cx: &mut Context<Self>) {
        if self.veil.is_secret() {
            edit.stop_history();
        }
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

    pub fn selected_text(&self) -> Option<String> {
        (!self.veil.is_secret() && !self.edit.selection().is_empty())
            .then(|| self.edit.selected_text().to_owned())
    }

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

    fn unit(m: &Modifiers, masked: bool) -> Unit {
        let unit = if m.platform {
            Unit::Line
        } else if m.alt {
            Unit::Word
        } else {
            Unit::Grapheme
        };
        if masked && unit != Unit::Grapheme {
            Unit::Document
        } else {
            unit
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
        let shortcut = is_shortcut(m);
        let edit = edit_shortcut(m);
        let letter = shortcut_letter(&k.key);
        match letter.as_str() {
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
            "backspace" => self.edit.delete_backward(Self::unit(m, self.veil.masks())),
            "delete" => self.edit.delete_forward(Self::unit(m, self.veil.masks())),
            "left" if cmd => self.row_edge(false, extend),
            "right" if cmd => self.row_edge(true, extend),
            "left" => self
                .edit
                .move_left(Self::unit(m, self.veil.masks()), extend),
            "right" => self
                .edit
                .move_right(Self::unit(m, self.veil.masks()), extend),
            "up" | "down" if self.arrows_pass => return,
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
            "a" if edit => self.apply(Command::SelectAll, cx),
            "c" if edit
                && route(
                    Command::Copy,
                    self.edit.selection().is_empty(),
                    self.veil.is_secret(),
                ) == Route::Pass =>
            {
                return
            }
            "x" if edit
                && route(
                    Command::Cut,
                    self.edit.selection().is_empty(),
                    self.veil.is_secret(),
                ) == Route::Pass =>
            {
                return
            }
            "c" if edit => self.apply(Command::Copy, cx),
            "x" if edit => self.apply(Command::Cut, cx),
            "v" if edit => self.apply(Command::Paste, cx),
            "z" if shortcut => self.apply(
                if m.shift {
                    Command::Redo
                } else {
                    Command::Undo
                },
                cx,
            ),
            _ => return,
        }
        if !vertical {
            self.goal_x = None;
        }
        self.touched(before != self.edit.text(), cx);
        cx.stop_propagation();
    }

    fn apply(&mut self, command: Command, cx: &mut Context<Self>) {
        match command {
            Command::SelectAll => self.edit.select_all(),
            Command::Copy => {
                if let Some(text) = self.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            Command::Cut => {
                if self.veil.is_secret() {
                    return;
                }
                if let Some(text) = self.edit.cut() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            Command::Paste => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let text = if self.multiline {
                        text
                    } else {
                        self.veil.single_line(&text)
                    };
                    self.edit.paste(&text);
                }
            }
            Command::Undo => {
                self.edit.undo();
            }
            Command::Redo => {
                self.edit.redo();
            }
        }
    }

    fn on_command(&mut self, command: Command, cx: &mut Context<Self>) {
        if !self.enabled {
            cx.propagate();
            return;
        }
        match route(
            command,
            self.edit.selection().is_empty(),
            self.veil.is_secret(),
        ) {
            Route::Run => {}
            Route::Swallow => return,
            Route::Pass => {
                cx.propagate();
                return;
            }
        }
        let before = self.edit.text().to_owned();
        self.apply(command, cx);
        self.goal_x = None;
        self.touched(before != self.edit.text(), cx);
    }

    fn vertical(&mut self, direction: i32, extend: bool) {
        let Some(layout) = self.layout.as_ref() else {
            return self.edit.move_to_edge(direction > 0, extend);
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

    fn row_edge(&mut self, end: bool, extend: bool) {
        let target = match self.layout.as_ref().filter(|_| !self.veil.masks()) {
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
                .on_action(
                    cx.listener(|this, _: &actions::Cut, _, cx| this.on_command(Command::Cut, cx)),
                )
                .on_action(
                    cx.listener(|this, _: &actions::Copy, _, cx| {
                        this.on_command(Command::Copy, cx)
                    }),
                )
                .on_action(cx.listener(|this, _: &actions::Paste, _, cx| {
                    this.on_command(Command::Paste, cx)
                }))
                .on_action(cx.listener(|this, _: &actions::SelectAll, _, cx| {
                    this.on_command(Command::SelectAll, cx)
                }))
                .on_action(
                    cx.listener(|this, _: &actions::Undo, _, cx| {
                        this.on_command(Command::Undo, cx)
                    }),
                )
                .on_action(
                    cx.listener(|this, _: &actions::Redo, _, cx| {
                        this.on_command(Command::Redo, cx)
                    }),
                )
                .on_scroll_wheel(cx.listener(Self::wheel))
                .into_any_element()
        } else {
            root.opacity(0.5).into_any_element()
        }
    }
}
