use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, AnyView, ClipboardItem, Context, Div, FocusHandle, KeyDownEvent, Render,
    ScrollHandle, SharedString, Window,
};
use notchling_kit::metric::{RADIUS_MD, SIDE_PADDING};
use notchling_kit::part::{row_look, ButtonKind, PartState};
use notchling_kit::scrollbar::GUTTER;
use notchling_kit::ui::{
    button, card, field, hsla, label, resize_edges, scrollable, window_appearance, KitStyled,
    TopBorder,
};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::model::{Outcome, PagesModel};
use super::renders::{model, NAME};
use super::window::show_page;
use crate::search::library::Library;
use crate::search::model::Press;
use crate::search::wire::Page;

const LIST_WIDTH: f32 = 300.0;
const REFRESH_EVERY: Duration = Duration::from_secs(3);
const OPENS_NOTE: &str =
    "Open Page shows the local copy in a window of its own. Nothing is sent to claude.ai.";

pub struct PagesView {
    pub model: PagesModel,
    library: Arc<Library>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    fixed: bool,
    pending: Option<String>,
    note: Option<String>,
}

impl PagesView {
    pub fn live(
        library: Arc<Library>,
        select: Option<String>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PagesView {
        let view = PagesView {
            model: PagesModel::new(),
            library,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            fixed: false,
            pending: select,
            note: None,
        };
        view.refresh(cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(REFRESH_EVERY).await;
            if this.update(cx, |view, cx| view.refresh(cx)).is_err() {
                break;
            }
        })
        .detach();
        view
    }

    pub fn fixed(model: PagesModel, cx: &mut Context<Self>) -> PagesView {
        PagesView {
            model,
            library: Arc::new(Library::fixed(Default::default())),
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            fixed: true,
            pending: None,
            note: None,
        }
    }

    fn refresh(&self, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        let library = self.library.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { library.pages() })
                .await;
            this.update(cx, |view, cx| {
                match result {
                    Ok(list) => {
                        view.model.set_list(list);
                        if let Some(key) = view.pending.take() {
                            view.model.select(&key);
                        }
                    }
                    Err(e) => view.note = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn select(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.model.pages().iter().any(|p| p.key() == key) {
            self.model.select(key);
            self.pending = None;
        } else {
            self.pending = Some(key.to_owned());
        }
        cx.notify();
    }

    fn open_page(&mut self, page: &Page, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        self.note = show_page(cx, page).err();
        cx.notify();
    }

    fn copy_link(&mut self, link: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(link));
        self.note = None;
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let press = Press {
            key: k.key.clone(),
            key_char: k.key_char.clone(),
            platform: k.modifiers.platform,
            control: k.modifiers.control,
            alt: k.modifiers.alt,
        };
        match self.model.press(&press) {
            Outcome::Moved => {
                let shown = self.model.shown();
                if let Some(ix) = shown
                    .iter()
                    .position(|p| Some(p.key()) == self.model.selection.as_deref())
                {
                    self.scroll.scroll_to_item(ix);
                }
            }
            Outcome::Open(page) => self.open_page(&page, cx),
            Outcome::Typed | Outcome::Cleared => {}
            Outcome::Ignored => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

fn hairline(a: Appearance) -> Div {
    div()
        .h(px(1.0))
        .w_full()
        .flex_shrink_0()
        .fill(Ink::Hairline, a)
}

fn page_row(page: &Page, selected: bool, a: Appearance) -> Div {
    let state = if selected {
        PartState::Selected
    } else {
        PartState::Rest
    };
    let hover = row_look(PartState::Hover, false, a);
    div()
        .w_full()
        .px(px(10.0))
        .py(px(8.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .rounded(px(RADIUS_MD))
        .look(&row_look(state, false, a))
        .when(!selected, |r| r.hover(move |s| s.look(&hover)))
        .child(label(&page.title, TypeRole::Heading, Ink::Text, a).elide())
        .child(
            label(
                &PagesModel::row_detail(page),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            )
            .elide(),
        )
        .when(!page.account.is_empty(), |r| {
            r.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .size(px(6.0))
                            .flex_shrink_0()
                            .rounded_full()
                            .fill(Ink::TextFaint, a),
                    )
                    .child(label(&page.account, TypeRole::Meta, Ink::TextMuted, a).elide()),
            )
        })
}

impl PagesView {
    fn list(&self, focused: bool, a: Appearance, cx: &mut Context<Self>) -> Div {
        let shown = self.model.shown();
        let mut rows = div()
            .id("pages-list")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p(px(8.0))
            .pr(px(GUTTER))
            .flex()
            .flex_col()
            .gap(px(2.0));
        for page in &shown {
            let selected = Some(page.key()) == self.model.selection.as_deref();
            let key = page.key().to_owned();
            let opened = (*page).clone();
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("page-{}", page.key())))
                    .w_full()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                        this.select(&key, cx);
                        if event.click_count() > 1 {
                            this.open_page(&opened, cx);
                        }
                    }))
                    .child(page_row(page, selected, a)),
            );
        }
        div()
            .w(px(LIST_WIDTH))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .fill(Ink::Sidebar, a)
            .border_r(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(
                div().p(px(12.0)).flex_shrink_0().child(
                    field(
                        &self.model.search,
                        "Search or Paste Artifact ID",
                        focused,
                        a,
                    )
                    .w_full(),
                ),
            )
            .child(hairline(a))
            .child(scrollable(rows).tracking(&self.scroll))
            .child(hairline(a))
            .child(div().p(px(8.0)).flex_shrink_0().child(label(
                &self.model.count_text(),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            )))
    }

    fn detail(&self, page: &Page, a: Appearance, cx: &mut Context<Self>) -> Div {
        let exists = self.fixed || Path::new(&page.path).is_file();
        let open_state = if exists {
            PartState::Rest
        } else {
            PartState::Disabled
        };
        let opened = page.clone();
        let open = button("open-page", "Open Page", ButtonKind::Primary, open_state, a).when(
            exists,
            |b| {
                b.on_click(cx.listener(move |this, _, _, cx| {
                    this.open_page(&opened, cx);
                }))
            },
        );
        let link = page.link.clone();
        let copy = (!link.is_empty()).then(|| {
            button(
                "copy-link",
                "Copy Link",
                ButtonKind::Secondary,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.copy_link(link.clone(), cx)))
        });
        let mut about = card(a)
            .p(px(SIDE_PADDING))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(label(OPENS_NOTE, TypeRole::Body, Ink::TextMuted, a));
        if exists {
            about = about.child(label(&page.path, TypeRole::Mono, Ink::TextFaint, a));
        } else {
            about = about.child(label(super::local::GONE, TypeRole::Body, Ink::Needs, a));
        }
        if !page.link.is_empty() {
            about = about.child(label(&page.link, TypeRole::Mono, Ink::TextFaint, a));
        }
        div()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(12.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(label(&page.title, TypeRole::Title, Ink::Text, a).elide())
                            .child(
                                label(
                                    &PagesModel::subtitle(page),
                                    TypeRole::Meta,
                                    Ink::TextMuted,
                                    a,
                                )
                                .elide(),
                            ),
                    )
                    .child(open)
                    .children(copy),
            )
            .child(hairline(a))
            .child(div().p(px(SIDE_PADDING)).child(about))
            .child(div().flex_1())
            .children(self.note.as_deref().map(|note| {
                label(note, TypeRole::Meta, Ink::Needs, a)
                    .px(px(SIDE_PADDING))
                    .py(px(10.0))
                    .border_t(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
            }))
    }
}

impl Render for PagesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        if !self.fixed && !self.focus.is_focused(window) {
            window.focus(&self.focus);
        }
        let focused = !self.fixed && self.focus.is_focused(window);
        let selected = self.model.selected().cloned();
        let right = match &selected {
            Some(page) => self.detail(page, a, cx),
            None => div()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .child(label(
                    self.model.empty_text(),
                    TypeRole::Body,
                    Ink::TextMuted,
                    a,
                )),
        };
        div()
            .id("pages-window")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key(event, cx)))
            .size_full()
            .flex()
            .fill(Ink::Window, a)
            .child(self.list(focused, a, cx))
            .child(right)
            .child(resize_edges(window, TopBorder::InTitleBar))
    }
}

pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    (name == NAME).then(|| cx.new(|cx| PagesView::fixed(model(), cx)).into())
}
