//! The search panel over GPUI (part U3, macOS): a search box, then the results grouped Pages and
//! Sessions, then a footer with the key hints. Every key goes through `SearchModel::press`; each
//! change of the query asks the daemon on the background executor, and an answer for an older
//! query is dropped. Return on a page opens the Artifacts window on it; on a session, Chat on it.
//! A render has fixed results and never calls anything or moves its window.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, AnyView, ClipboardItem, Context, Div, FocusHandle, FontWeight,
    HighlightStyle, KeyDownEvent, Render, ScrollHandle, SharedString, Stateful, StyledText,
    Subscription, Window,
};
use notchling_engine::search::{age_text, SessionTool};
use notchling_kit::metric::{RADIUS_CARD, RADIUS_MD, RADIUS_SM};
use notchling_kit::ui::{claude_mark, hsla, label, window_appearance, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use objc2::rc::Retained;
use objc2_app_kit::NSWindow;

use super::library::Library;
use super::model::{layout, Item, Outcome, Press, SearchModel, Section};
use super::panel;
use super::renders::{model, results, shown};
use super::wire::{Page, SessionRow};
use crate::chat::format::short_path;

/// How often the window is put back where it belongs (GPUI moves it after opening).
const PLACE_EVERY: Duration = Duration::from_millis(100);
/// How long after an answer that said `indexing` the same query is asked again.
const INDEXING_RETRY: Duration = Duration::from_millis(700);

pub struct SearchPanelView {
    pub model: SearchModel,
    library: Arc<Library>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// Bumped per search; an answer for an older one is dropped.
    generation: u64,
    /// A render's: fixed results, nothing called, the window left alone.
    fixed: bool,
    ns_window: Option<Retained<NSWindow>>,
    /// Where the panel hangs from (AppKit coordinates).
    top: (f64, f64),
    /// It became key once: a later deactivation is a click elsewhere, which closes it.
    seen_active: bool,
    _activation: Option<Subscription>,
}

fn now_ms() -> i64 {
    crate::phone::bridge::now_ms()
}

impl SearchPanelView {
    /// The live panel: searches at once (the newest pages and sessions), closes itself when the
    /// window stops being key, and keeps its window placed.
    pub fn live(
        library: Arc<Library>,
        ns_window: Option<Retained<NSWindow>>,
        top: (f64, f64),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> SearchPanelView {
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.seen_active = true;
            } else if this.seen_active {
                // Clicked elsewhere: that click already chose where focus goes.
                panel::close(cx, window, false);
            }
        });
        let mut view = SearchPanelView {
            model: SearchModel::new(),
            library,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            generation: 0,
            fixed: false,
            ns_window,
            top,
            seen_active: false,
            _activation: Some(activation),
        };
        view.search(cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(PLACE_EVERY).await;
            if this.update(cx, |view, _| view.place()).is_err() {
                break;
            }
        })
        .detach();
        view
    }

    /// A render's: `model` as given, nothing behind it.
    pub fn fixed(model: SearchModel, library: Arc<Library>, cx: &mut Context<Self>) -> Self {
        SearchPanelView {
            model,
            library,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            generation: 0,
            fixed: true,
            ns_window: None,
            top: (0.0, 0.0),
            seen_active: false,
            _activation: None,
        }
    }

    fn place(&self) {
        if let (false, Some(w)) = (self.fixed, &self.ns_window) {
            panel::place(w, self.top, self.model.height());
        }
    }

    /// Asks the daemon for the query as it is now; the answer lands unless the query moved on.
    fn search(&mut self, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.model.searching = true;
        let (library, text, limit) = (
            self.library.clone(),
            self.model.query.clone(),
            self.model.limit(),
        );
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { library.search(&text, limit) })
                .await;
            this.update(cx, |view, cx| {
                if view.generation != generation {
                    return;
                }
                view.model.searching = false;
                let mut again = false;
                match result {
                    Ok(results) => {
                        again = results.indexing;
                        let keeping = view.model.current().map(Item::id);
                        view.model.apply(results, keeping.as_deref());
                        view.model.error = None;
                    }
                    Err(e) => view.model.error = Some(e),
                }
                view.scroll_to_selected();
                if again {
                    // The daemon is reading the index: the same query may find more in a moment.
                    view.search_later(generation, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Searches again after a moment, unless the query moved on meanwhile.
    fn search_later(&self, generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(INDEXING_RETRY).await;
            this.update(cx, |view, cx| {
                if view.generation == generation {
                    view.search(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The list's children are headers and rows; the selected row's index among them.
    fn scroll_to_selected(&self) {
        let selected = self.model.selected;
        let headers = (0..=selected)
            .filter(|i| self.model.starts_section(*i))
            .count();
        self.scroll.scroll_to_item(selected + headers);
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let press = Press {
            key: k.key.clone(),
            key_char: k.key_char.clone(),
            platform: k.modifiers.platform,
            control: k.modifiers.control,
            alt: k.modifiers.alt,
        };
        match self.model.press(&press) {
            Outcome::Moved => self.scroll_to_selected(),
            Outcome::Open(item) => self.open(item, window, cx),
            Outcome::Hide { returning } => {
                if !self.fixed {
                    panel::close(cx, window, returning);
                }
            }
            Outcome::Cleared | Outcome::Typed => self.search(cx),
            Outcome::Copy { text, note } => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.model.flash(note, now_ms());
            }
            Outcome::Ignored => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Opens the row: a page in the Artifacts window, a session in Chat. The panel closes first
    /// and doesn't give focus back, since the window opened takes it.
    fn open(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        panel::close(cx, window, false);
        match item {
            Item::Page(page) => crate::pages::window::open(cx, Some(page.key().to_owned())),
            Item::Session(session) => crate::mac::shell::open_chat_session(cx, session.reference()),
        }
    }
}

fn hairline(a: Appearance) -> Div {
    div()
        .h(px(1.0))
        .w_full()
        .flex_shrink_0()
        .fill(Ink::Hairline, a)
}

/// The 30-point square a row's icon sits in.
fn icon_box(selected: bool, a: Appearance) -> Div {
    div()
        .size(px(30.0))
        .flex_shrink_0()
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .fill(
            if selected {
                Ink::AccentWash
            } else {
                Ink::Raised
            },
            a,
        )
}

/// A tool's mark: Claude's sparkle, else its short name.
fn tool_mark(tool: SessionTool, selected: bool, a: Appearance) -> Div {
    let ink = if selected {
        Ink::AccentText
    } else {
        Ink::TextMuted
    };
    match tool {
        SessionTool::Claude => icon_box(selected, a).child(claude_mark(20.0, a)),
        SessionTool::Codex => icon_box(selected, a).child(label("Cx", TypeRole::Label, ink, a)),
        SessionTool::Antigravity => {
            icon_box(selected, a).child(label("Ag", TypeRole::Label, ink, a))
        }
    }
}

fn section_header(section: Section, a: Appearance) -> Div {
    div()
        .h(px(layout::HEADER))
        .flex_shrink_0()
        .px(px(10.0))
        .pb(px(4.0))
        .flex()
        .items_end()
        .child(label(section.title(), TypeRole::Section, Ink::TextFaint, a))
}

/// A row's frame: the selection wash when selected.
fn row_frame(height: f32, selected: bool, a: Appearance) -> Div {
    div()
        .h(px(height))
        .w_full()
        .flex_shrink_0()
        .px(px(10.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .rounded(px(RADIUS_MD))
        .when(selected, |r| r.fill(Ink::Selection, a))
}

fn page_row(page: &Page, selected: bool, now_secs: f64, a: Appearance) -> Div {
    let subtitle: Vec<&str> = [page.project.as_str(), page.account.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    row_frame(layout::PAGE_ROW, selected, a)
        .child(icon_box(selected, a).child(label(
            Icon::Page.glyph(),
            TypeRole::Heading,
            if selected {
                Ink::AccentText
            } else {
                Ink::TextMuted
            },
            a,
        )))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(label(&page.title, TypeRole::Heading, Ink::Text, a).truncate())
                .child(label(&subtitle.join(" · "), TypeRole::Meta, Ink::TextMuted, a).truncate()),
        )
        .child(label(
            &age_text(page.updated_secs, now_secs),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ))
}

/// The snippet with the query's words bold, else the folder.
fn session_detail(session: &SessionRow, home: &std::path::Path, a: Appearance) -> Div {
    let line = div()
        .kit_type(TypeRole::Meta)
        .ink(Ink::TextMuted, a)
        .truncate();
    match &session.snippet {
        Some(snippet) => {
            let bold = HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                color: Some(hsla(Ink::Text.color(a))),
                ..Default::default()
            };
            let ranges = session
                .highlights
                .iter()
                .filter(|(s, e)| {
                    s < e && snippet.is_char_boundary(*s) && snippet.is_char_boundary(*e)
                })
                .map(|(s, e)| (Range { start: *s, end: *e }, bold));
            line.child(StyledText::new(SharedString::from(snippet.clone())).with_highlights(ranges))
        }
        None => line.child(short_path(&session.folder, home)),
    }
}

fn session_row(
    session: &SessionRow,
    selected: bool,
    now_secs: f64,
    home: &std::path::Path,
    a: Appearance,
) -> Div {
    let place = std::path::Path::new(&session.folder)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_owned();
    row_frame(layout::SESSION_ROW, selected, a)
        .child(tool_mark(session.tool, selected, a))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            label(&session.title, TypeRole::Heading, Ink::Text, a)
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate(),
                        )
                        .child(label(&place, TypeRole::Meta, Ink::TextMuted, a).truncate())
                        .child(label(
                            &age_text(session.updated_secs, now_secs),
                            TypeRole::Meta,
                            Ink::TextFaint,
                            a,
                        )),
                )
                .child(session_detail(session, home, a)),
        )
}

impl SearchPanelView {
    fn field(&self, a: Appearance) -> Div {
        let empty = self.model.query.is_empty();
        let text = if empty {
            "Search Pages and Sessions".to_owned()
        } else {
            self.model.query.clone()
        };
        div()
            .h(px(layout::FIELD))
            .flex_shrink_0()
            .px(px(18.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(label(
                Icon::Search.glyph(),
                TypeRole::Title,
                Ink::TextMuted,
                a,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .child(
                        label(
                            &text,
                            TypeRole::Title,
                            if empty { Ink::TextFaint } else { Ink::Text },
                            a,
                        )
                        .font_weight(FontWeight::NORMAL)
                        .truncate(),
                    )
                    // The caret, after the text (the field takes every key while the panel is up).
                    .child(
                        div()
                            .w(px(1.5))
                            .h(px(22.0))
                            .ml(px(1.0))
                            .fill(Ink::Accent, a),
                    ),
            )
    }

    /// The clock the ages are read against: a render's is fixed, like its data.
    fn now_secs(&self) -> f64 {
        if self.fixed {
            super::renders::NOW_SECS
        } else {
            now_ms() as f64 / 1000.0
        }
    }

    fn results(&self, a: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        let now_secs = self.now_secs();
        let home = notchling_platform::paths::home().unwrap_or_default();
        let mut list = div()
            .id("search-results")
            .track_scroll(&self.scroll)
            .h(px(self.model.list_height() + 2.0 * layout::LIST_PADDING))
            .flex_shrink_0()
            .overflow_y_scroll()
            .p(px(layout::LIST_PADDING))
            .flex()
            .flex_col();
        for (index, item) in self.model.items().iter().enumerate() {
            if self.model.starts_section(index) {
                list = list.child(section_header(item.section(), a));
            }
            let selected = index == self.model.selected;
            let row = match item {
                Item::Page(page) => page_row(page, selected, now_secs, a),
                Item::Session(session) => session_row(session, selected, now_secs, &home, a),
            };
            let opened = item.clone();
            list =
                list.child(
                    div()
                        .id(SharedString::from(format!("search-row-{index}")))
                        .w_full()
                        .cursor_pointer()
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered && this.model.selected != index {
                                this.model.selected = index;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open(opened.clone(), window, cx)
                        }))
                        .child(row),
                );
        }
        list
    }

    fn footer(&self, now: i64, a: Appearance) -> Div {
        let note = self.model.note(now).is_some();
        let text = self.model.footer_text(now);
        let mut footer = div()
            .h(px(layout::FOOTER))
            .flex_shrink_0()
            .px(px(16.0))
            .flex()
            .items_center()
            .gap(px(14.0))
            .child(if note {
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .child(label(
                        Icon::Check.glyph(),
                        TypeRole::Meta,
                        Ink::AccentText,
                        a,
                    ))
                    .child(label(&text, TypeRole::Meta, Ink::AccentText, a))
            } else {
                label(&text, TypeRole::Meta, Ink::TextFaint, a).truncate()
            })
            .child(div().flex_1());
        for (key, what) in self.model.hints() {
            footer = footer.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .child(label(key, TypeRole::Meta, Ink::TextMuted, a))
                    .child(label(what, TypeRole::Meta, Ink::TextFaint, a)),
            );
        }
        footer
    }
}

impl Render for SearchPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        if !self.fixed && !self.focus.is_focused(window) {
            window.focus(&self.focus);
        }
        self.place();
        let now = now_ms();
        let body = if self.model.items().is_empty() {
            div()
                .h(px(layout::EMPTY))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(label(
                    self.model.empty_text(),
                    TypeRole::Body,
                    Ink::TextMuted,
                    a,
                ))
                .into_any_element()
        } else {
            self.results(a, cx).into_any_element()
        };
        div()
            .id("search-panel")
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| this.key(event, window, cx)),
            )
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .overflow_hidden()
            .border(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .fill(Ink::Window, a)
            .child(self.field(a))
            .child(hairline(a))
            .child(body)
            .child(hairline(a))
            .child(self.footer(now, a))
    }
}

/// `--render search-panel[-results]`, over made-up results; nothing is called.
pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    let which = shown(name)?;
    let library = Arc::new(Library::fixed(results(which)));
    Some(
        cx.new(|cx| SearchPanelView::fixed(model(which), library, cx))
            .into(),
    )
}
