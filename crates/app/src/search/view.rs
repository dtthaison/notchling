use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, AnyView, ClipboardItem, Context, Div, Entity, FocusHandle, FontWeight,
    KeyDownEvent, Render, ScrollHandle, SharedString, Subscription, Window,
};
use notchling_kit::metric::RADIUS_CARD;
use notchling_kit::scrollbar::GUTTER;
use notchling_kit::ui::{
    hsla, icon, label, scrollable, window_appearance, KitStyled, Scrollable, TextEditor,
    TextEditorEvent,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use objc2::rc::Retained;
use objc2_app_kit::NSWindow;

use super::library::Library;
use super::model::{layout, Item, Outcome, Press, SearchModel};
use super::pacer::{Pacer, Settled, DEBOUNCE};
use super::panel;
use super::renders::{model, results, shown};
use super::rows::{hairline, page_row, secret_row, section_header, session_row};

const SEARCH_PLACEHOLDER: &str = "Search Pages and Sessions";
const PLACE_EVERY: Duration = Duration::from_millis(100);
const INDEXING_RETRY: Duration = Duration::from_millis(700);

pub struct SearchPanelView {
    pub model: SearchModel,
    library: Arc<Library>,
    focus: FocusHandle,
    query: Entity<TextEditor>,
    scroll: ScrollHandle,
    pacer: Pacer,
    fixed: bool,
    ns_window: Option<Retained<NSWindow>>,
    top: (f64, f64),
    seen_active: bool,
    _activation: Option<Subscription>,
    _query_events: Subscription,
}

fn now_ms() -> i64 {
    crate::phone::bridge::now_ms()
}

impl SearchPanelView {
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
                panel::close(cx, window, false);
            }
        });
        let query = cx.new(|cx| {
            TextEditor::new(SEARCH_PLACEHOLDER, 1, cx)
                .single_line()
                .arrows_pass()
        });
        let events = cx.subscribe_in(
            &query,
            window,
            |this, _, event: &TextEditorEvent, window, cx| match event {
                TextEditorEvent::Changed => this.query_changed(cx),
                TextEditorEvent::Submit => {
                    if let Some(item) = this.model.current().cloned() {
                        this.open(item, window, cx);
                    }
                }
            },
        );
        let mut view = SearchPanelView {
            model: SearchModel::new(),
            library,
            focus: cx.focus_handle(),
            query,
            scroll: ScrollHandle::new(),
            pacer: Pacer::default(),
            fixed: false,
            ns_window,
            top,
            seen_active: false,
            _activation: Some(activation),
            _query_events: events,
        };
        view.search(Duration::ZERO, cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(PLACE_EVERY).await;
            if this.update(cx, |view, _| view.place()).is_err() {
                break;
            }
        })
        .detach();
        view
    }

    pub fn fixed(model: SearchModel, library: Arc<Library>, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| {
            TextEditor::new(SEARCH_PLACEHOLDER, 1, cx)
                .single_line()
                .preview()
        });
        query.update(cx, |editor, cx| {
            editor.set_text(&model.query, cx);
            if std::env::var_os("NOTCHLING_RENDER_SELECT").is_some() {
                editor.preview_selection(0, model.query.len(), cx);
            }
        });
        let events = cx.subscribe(&query, |this, _, event: &TextEditorEvent, cx| {
            if matches!(event, TextEditorEvent::Changed) {
                this.query_changed(cx);
            }
        });
        SearchPanelView {
            model,
            library,
            focus: cx.focus_handle(),
            query,
            scroll: ScrollHandle::new(),
            pacer: Pacer::default(),
            fixed: true,
            ns_window: None,
            top: (0.0, 0.0),
            seen_active: false,
            _activation: None,
            _query_events: events,
        }
    }

    fn query_changed(&mut self, cx: &mut Context<Self>) {
        let text = self.query.read(cx).text().to_owned();
        if self.model.query != text {
            self.model.query = text;
        }
        self.search(DEBOUNCE, cx);
    }

    fn place(&self) {
        if let (false, Some(w)) = (self.fixed, &self.ns_window) {
            panel::place(w, self.top, self.model.height());
        }
    }

    fn search(&mut self, after: Duration, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        let generation = self.pacer.typed();
        self.model.searching = true;
        if after.is_zero() {
            self.settled(generation, cx);
        } else {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(after).await;
                this.update(cx, |view, cx| view.settled(generation, cx))
                    .ok();
            })
            .detach();
        }
        cx.notify();
    }

    fn settled(&mut self, generation: u64, cx: &mut Context<Self>) {
        if let Settled::Ask(generation) = self.pacer.settled(generation) {
            self.ask(generation, cx);
        }
    }

    fn ask(&mut self, generation: u64, cx: &mut Context<Self>) {
        let (library, text, limit) = (
            self.library.clone(),
            self.model.query.clone(),
            self.model.limit(),
        );
        let vault = cx
            .try_global::<crate::mac::shell::AppVault>()
            .map(|v| v.0.clone());
        let switches = crate::accounts::window::app_stores(cx).1.switches();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    library.search(&text, limit).map(|mut results| {
                        if let (Some(vault), false) = (vault, text.trim().is_empty()) {
                            vault.refresh(&switches);
                            let entries = vault.snapshot().entries;
                            results.secrets = notchling_engine::vault::list::find(&text, &entries);
                        }
                        results
                    })
                })
                .await;
            this.update(cx, |view, cx| {
                let answered = view.pacer.finished(generation);
                if answered.apply {
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
                        view.search_later(generation, cx);
                    }
                }
                if let Some(next) = answered.next {
                    view.ask(next, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn search_later(&self, generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(INDEXING_RETRY).await;
            this.update(cx, |view, cx| {
                if view.pacer.generation() == generation {
                    view.search(Duration::ZERO, cx);
                }
            })
            .ok();
        })
        .detach();
    }

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
            Outcome::Cleared => {
                self.query.update(cx, |editor, cx| editor.set_text("", cx));
                self.search(DEBOUNCE, cx);
            }
            Outcome::Copy { text, note } => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.model.flash(note, now_ms());
            }
            Outcome::Ignored => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn open(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        panel::close(cx, window, false);
        match item {
            Item::Page(page) => crate::pages::window::open(cx, Some(page.key().to_owned())),
            Item::Secret(entry) => crate::mac::shell::open_chat_secret(cx, &entry.name),
            Item::Session(session) => {
                crate::mac::shell::open_chat_session(cx, session.reference(), session.child())
            }
        }
    }
}

impl SearchPanelView {
    fn field(&self, a: Appearance) -> Div {
        div()
            .h(px(layout::FIELD))
            .flex_shrink_0()
            .px(px(18.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(icon(
                Icon::Search,
                TypeRole::Title.spec().size,
                Ink::TextMuted,
                a,
            ))
            .child(
                div().flex_1().min_w(px(0.0)).flex().items_center().child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .kit_type(TypeRole::Title)
                        .ink(Ink::Text, a)
                        .font_weight(FontWeight::NORMAL)
                        .child(self.query.clone()),
                ),
            )
    }

    fn now_secs(&self) -> f64 {
        if self.fixed {
            super::renders::NOW_SECS
        } else {
            now_ms() as f64 / 1000.0
        }
    }

    fn results(&self, a: Appearance, cx: &mut Context<Self>) -> Scrollable {
        let now_secs = self.now_secs();
        let home = notchling_platform::paths::home().unwrap_or_default();
        let mut list = div()
            .id("search-results")
            .h(px(self.model.list_height() + 2.0 * layout::LIST_PADDING))
            .flex_shrink_0()
            .overflow_y_scroll()
            .p(px(layout::LIST_PADDING))
            .pr(px(GUTTER))
            .flex()
            .flex_col();
        for (index, item) in self.model.items().iter().enumerate() {
            if self.model.starts_section(index) {
                list = list.child(section_header(item.section(), a));
            }
            let selected = index == self.model.selected;
            let row = match item {
                Item::Page(page) => page_row(page, selected, now_secs, a),
                Item::Secret(entry) => secret_row(entry, selected, a),
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
        scrollable(list).tracking(&self.scroll)
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
                    .child(icon(
                        Icon::Check,
                        TypeRole::Meta.spec().size,
                        Ink::AccentText,
                        a,
                    ))
                    .child(label(&text, TypeRole::Meta, Ink::AccentText, a))
            } else {
                label(&text, TypeRole::Meta, Ink::TextFaint, a).elide()
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
        if !self.fixed && !self.query.read(cx).is_focused(window) {
            self.query.read(cx).focus(window);
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

pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    let which = shown(name)?;
    let library = Arc::new(Library::fixed(results(which)));
    Some(
        cx.new(|cx| SearchPanelView::fixed(model(which), library, cx))
            .into(),
    )
}
