use std::ops::Range;

use gpui::{
    fill, point, px, relative, size, App, AvailableSpace, Bounds, ContentMask, CursorStyle,
    DispatchPhase, Element, ElementId, ElementInputHandler, Entity, Font, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, Style, TextAlign, TextRun,
    UnderlineStyle, Window, WrappedLine,
};

use super::TextEditor;
use crate::ui::{hsla, window_appearance};
use crate::Ink;

const CARET: f32 = 1.5;
const NEWLINE_MARK: f32 = 5.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub paragraph: usize,
    pub start: usize,
    pub end: usize,
    pub y: Pixels,
}

#[derive(Clone)]
pub struct Layout {
    pub lines: Vec<WrappedLine>,
    pub paragraphs: Vec<usize>,
    pub rows: Vec<Row>,
    pub line_height: Pixels,
    pub bounds: Bounds<Pixels>,
    pub scroll_top: Pixels,
}

impl Layout {
    pub fn content_height(&self) -> Pixels {
        self.line_height * self.rows.len() as f32
    }

    pub fn max_scroll(&self) -> Pixels {
        (self.content_height() - self.bounds.size.height).max(px(0.0))
    }

    pub fn row_at(&self, index: usize) -> Row {
        self.rows
            .iter()
            .find(|row| index <= row.end)
            .or(self.rows.last())
            .cloned()
            .unwrap_or(Row {
                paragraph: 0,
                start: 0,
                end: 0,
                y: px(0.0),
            })
    }

    fn x_on_row(&self, row: &Row, index: usize) -> Pixels {
        if index <= row.start {
            return px(0.0);
        }
        let Some(line) = self.lines.get(row.paragraph) else {
            return px(0.0);
        };
        let local = index - self.paragraphs.get(row.paragraph).copied().unwrap_or(0);
        line.position_for_index(local, self.line_height)
            .map(|p| p.x)
            .unwrap_or(px(0.0))
    }

    pub fn position_for_index(&self, index: usize) -> Point<Pixels> {
        let row = self.row_at(index);
        point(self.x_on_row(&row, index), row.y)
    }

    pub fn index_for_point(&self, p: Point<Pixels>) -> usize {
        let Some(last) = self.rows.last() else {
            return 0;
        };
        let row_ix = (f32::from(p.y) / f32::from(self.line_height)).floor();
        let row_ix = (row_ix.max(0.0) as usize).min(self.rows.len() - 1);
        let Some(row) = self.rows.get(row_ix) else {
            return last.end;
        };
        let Some(line) = self.lines.get(row.paragraph) else {
            return row.end;
        };
        let start = self.paragraphs.get(row.paragraph).copied().unwrap_or(0);
        let first_row = self
            .rows
            .iter()
            .position(|r| r.paragraph == row.paragraph)
            .unwrap_or(row_ix);
        let local_y = self.line_height * (row_ix - first_row) as f32 + self.line_height / 2.0;
        let local = point(p.x.max(px(0.0)), local_y);
        match line.closest_index_for_position(local, self.line_height) {
            Ok(i) | Err(i) => start + i,
        }
    }

    pub fn selection_quads(&self, selection: &Range<usize>) -> Vec<Bounds<Pixels>> {
        let mut quads = Vec::new();
        for row in &self.rows {
            if selection.end < row.start || selection.start > row.end || selection.is_empty() {
                continue;
            }
            let from = self.x_on_row(row, selection.start.max(row.start));
            let mut to = self.x_on_row(row, selection.end.min(row.end));
            let wrapped = self
                .rows
                .iter()
                .any(|r| r.start == row.end && r.end > row.end);
            if selection.end > row.end && !wrapped {
                to += px(NEWLINE_MARK);
            }
            if to > from {
                quads.push(Bounds::new(
                    point(from, row.y),
                    size(to - from, self.line_height),
                ));
            }
        }
        quads
    }
}

fn shape(
    window: &Window,
    text: &SharedString,
    font: &Font,
    font_size: Pixels,
    color: Hsla,
    marked: Option<Range<usize>>,
    wrap_width: Option<Pixels>,
) -> Vec<WrappedLine> {
    let run = |len: usize, underline: bool| TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: underline.then_some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: None,
    };
    let runs = match marked.filter(|m| m.end <= text.len() && m.start < m.end) {
        Some(m) => vec![
            run(m.start, false),
            run(m.end - m.start, true),
            run(text.len() - m.end, false),
        ],
        None => vec![run(text.len(), false)],
    };
    window
        .text_system()
        .shape_text(text.clone(), font_size, &runs, wrap_width, None)
        .map(|lines| lines.into_vec())
        .unwrap_or_default()
}

fn rows(lines: &[WrappedLine], paragraphs: &[usize], line_height: Pixels) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut y = px(0.0);
    for (paragraph, line) in lines.iter().enumerate() {
        let start = paragraphs.get(paragraph).copied().unwrap_or(0);
        let mut row_start = 0;
        for boundary in line.wrap_boundaries() {
            let Some(glyph) = line
                .runs()
                .get(boundary.run_ix)
                .and_then(|run| run.glyphs.get(boundary.glyph_ix))
            else {
                continue;
            };
            rows.push(Row {
                paragraph,
                start: start + row_start,
                end: start + glyph.index,
                y,
            });
            y += line_height;
            row_start = glyph.index;
        }
        rows.push(Row {
            paragraph,
            start: start + row_start,
            end: start + line.len(),
            y,
        });
        y += line_height;
    }
    rows
}

fn paragraph_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

struct Shown {
    text: SharedString,
    placeholder: bool,
    marked: Option<Range<usize>>,
    max_lines: usize,
}

pub(super) struct TextElement {
    editor: Entity<TextEditor>,
}

pub(super) struct Prepainted {
    layout: Layout,
    hitbox: Hitbox,
}

impl TextElement {
    pub fn new(editor: Entity<TextEditor>) -> TextElement {
        TextElement { editor }
    }

    fn shown(&self, cx: &App) -> Shown {
        let editor = self.editor.read(cx);
        let placeholder = editor.edit.is_empty();
        Shown {
            text: if placeholder {
                editor.placeholder.clone()
            } else {
                SharedString::from(editor.edit.text().to_owned())
            },
            placeholder,
            marked: editor.edit.marked(),
            max_lines: editor.max_lines,
        }
    }

    fn color(window: &Window, placeholder: bool) -> Hsla {
        if placeholder {
            hsla(Ink::TextFaint.color(window_appearance(window)))
        } else {
            window.text_style().color
        }
    }

    fn listen_mouse(&self, hitbox: Hitbox, window: &mut Window) {
        let editor = self.editor.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && event.button == MouseButton::Left
                && hitbox.is_hovered(window)
            {
                editor.update(cx, |editor, cx| editor.mouse_down(event, window, cx));
            }
        });
        let editor = self.editor.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && event.pressed_button == Some(MouseButton::Left) {
                editor.update(cx, |editor, cx| editor.mouse_move(event, cx));
            }
        });
        let editor = self.editor.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                editor.update(cx, |editor, cx| editor.mouse_up(cx));
            }
        });
    }
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

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
        let shown = self.shown(cx);
        let style = window.text_style();
        let rem = window.rem_size();
        let line_height = style.line_height_in_pixels(rem);
        let font_size = style.font_size.to_pixels(rem);
        let font = style.font();
        let max_lines = if shown.max_lines == 0 {
            usize::MAX
        } else {
            shown.max_lines
        };
        let mut layout_style = Style::default();
        layout_style.size.width = relative(1.0).into();
        let id =
            window.request_measured_layout(layout_style, move |known, available, window, _| {
                let width = known.width.or(match available.width {
                    AvailableSpace::Definite(w) => Some(w),
                    _ => None,
                });
                let lines = shape(
                    window,
                    &shown.text,
                    &font,
                    font_size,
                    Hsla::default(),
                    None,
                    width,
                );
                let count: usize = lines.iter().map(|l| l.wrap_boundaries().len() + 1).sum();
                let count = count.clamp(1, max_lines);
                let widest = lines
                    .iter()
                    .map(|l| l.width())
                    .fold(px(0.0), |a, b| a.max(b));
                size(width.unwrap_or(widest), line_height * count as f32)
            });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepainted {
        let shown = self.shown(cx);
        let style = window.text_style();
        let rem = window.rem_size();
        let line_height = style.line_height_in_pixels(rem);
        let font_size = style.font_size.to_pixels(rem);
        let color = Self::color(window, shown.placeholder);
        let lines = shape(
            window,
            &shown.text,
            &style.font(),
            font_size,
            color,
            shown.marked,
            Some(bounds.size.width),
        );
        let editor = self.editor.read(cx);
        let paragraphs = if shown.placeholder {
            vec![0]
        } else {
            paragraph_starts(editor.edit.text())
        };
        let rows = rows(&lines, &paragraphs, line_height);
        let (caret, reveal, mut scroll_top) =
            (editor.edit.caret(), editor.reveal_caret, editor.scroll_top);
        let mut layout = Layout {
            lines,
            paragraphs,
            rows,
            line_height,
            bounds,
            scroll_top: px(0.0),
        };
        scroll_top = scroll_top.clamp(px(0.0), layout.max_scroll());
        if reveal {
            let caret_y = layout.position_for_index(caret).y;
            if caret_y < scroll_top {
                scroll_top = caret_y;
            } else if caret_y + line_height > scroll_top + bounds.size.height {
                scroll_top = caret_y + line_height - bounds.size.height;
            }
        }
        layout.scroll_top = scroll_top;
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let kept = layout.clone();
        self.editor.update(cx, |editor, _| {
            editor.layout = Some(kept);
            editor.scroll_top = scroll_top;
            editor.reveal_caret = false;
        });
        Prepainted { layout, hitbox }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepainted: &mut Prepainted,
        window: &mut Window,
        cx: &mut App,
    ) {
        let editor = self.editor.read(cx);
        let focus = editor.focus.clone();
        let enabled = editor.enabled;
        let shows_caret = enabled && (editor.preview || editor.is_focused(window));
        let caret_on = editor.caret_on || editor.preview;
        let (selection, caret) = (editor.edit.selection(), editor.edit.caret());
        if enabled {
            window.handle_input(
                &focus,
                ElementInputHandler::new(bounds, self.editor.clone()),
                cx,
            );
            window.set_cursor_style(CursorStyle::IBeam, &prepainted.hitbox);
        }
        let a = window_appearance(window);
        let layout = &prepainted.layout;
        let origin = point(bounds.left(), bounds.top() - layout.scroll_top);
        let shift = |b: Bounds<Pixels>| Bounds::new(b.origin + origin, b.size);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if shows_caret {
                let color = hsla(Ink::Accent.color(a).with_alpha(0.35));
                for quad in layout.selection_quads(&selection) {
                    window.paint_quad(fill(shift(quad), color));
                }
            }
            for (paragraph, line) in layout.lines.iter().enumerate() {
                let y = layout
                    .rows
                    .iter()
                    .find(|r| r.paragraph == paragraph)
                    .map(|r| r.y)
                    .unwrap_or(px(0.0));
                line.paint(
                    point(origin.x, origin.y + y),
                    layout.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
            }
            if shows_caret && selection.is_empty() && caret_on {
                let at = layout.position_for_index(caret);
                let caret = Bounds::new(at, size(px(CARET), layout.line_height));
                window.paint_quad(fill(shift(caret), hsla(Ink::Accent.color(a))));
            }
        });
        if enabled {
            self.listen_mouse(prepainted.hitbox.clone(), window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_start_after_each_newline() {
        assert_eq!(paragraph_starts(""), vec![0]);
        assert_eq!(paragraph_starts("one"), vec![0]);
        assert_eq!(paragraph_starts("one\ntwo\n"), vec![0, 4, 8]);
    }

    fn layout(rows: &[(usize, usize, usize)]) -> Layout {
        let line_height = px(16.0);
        Layout {
            lines: Vec::new(),
            paragraphs: vec![0],
            rows: rows
                .iter()
                .enumerate()
                .map(|(i, &(paragraph, start, end))| Row {
                    paragraph,
                    start,
                    end,
                    y: line_height * i as f32,
                })
                .collect(),
            line_height,
            bounds: Bounds::new(point(px(0.0), px(0.0)), size(px(200.0), px(32.0))),
            scroll_top: px(0.0),
        }
    }

    #[test]
    fn a_wrap_belongs_to_the_row_before_it_and_a_newline_to_the_row_after() {
        let l = layout(&[(0, 0, 5), (0, 5, 9), (1, 10, 13)]);
        assert_eq!(l.row_at(0).start, 0);
        assert_eq!(l.row_at(5).y, px(0.0), "at the wrap: the first row's end");
        assert_eq!(l.row_at(6).y, px(16.0));
        assert_eq!(l.row_at(9).y, px(16.0), "before the newline");
        assert_eq!(l.row_at(10).y, px(32.0), "after it");
        assert_eq!(l.row_at(99).y, px(32.0), "past the end: the last row");
        assert_eq!(l.content_height(), px(48.0));
        assert_eq!(l.max_scroll(), px(16.0));
    }

    #[test]
    fn selection_quads_cover_each_row_touched_and_mark_a_selected_newline() {
        let l = layout(&[(0, 0, 5), (0, 5, 9), (1, 10, 13)]);
        assert!(l.selection_quads(&(0..0)).is_empty());
        let across = l.selection_quads(&(7..12));
        assert_eq!(
            across.len(),
            1,
            "the wrap row gets no mark, the newline row does"
        );
        assert_eq!(across[0].origin.y, px(16.0));
        assert_eq!(across[0].size.width, px(NEWLINE_MARK));
    }
}
