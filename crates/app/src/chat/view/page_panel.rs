use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::chat::run_command::RunHow;
use gpui::{
    canvas, div, img, prelude::*, px, AnyElement, App, Context, CursorStyle, DragMoveEvent,
    EmptyView, ImageFormat, ObjectFit, SharedString, WeakEntity, Window,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::RADIUS_MD;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, icon, icon_button, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::diff_panel::{self, DiffState};
use super::render_map::WindowRegion;
use super::ChatView;
use crate::chat::file_panel::{self, Body, Loaded, Pic};
use crate::chat::html_preview::HtmlView;
use crate::chat::markdown_cache::Cache as MarkdownCache;
use crate::chat::page_panel::{
    artifact_link, decide, dragged_width, panel_width, subtitle, ArtifactLink, Frame, Opens,
    DEFAULT_WIDTH,
};
use crate::mac::gate;
use crate::pages::local;
use crate::pages::web::{PanelHooks, PanelPage, PanelWeb};
use crate::pages::window::AppPagesDir;
use crate::search::panel::app_library;

pub const PAGE_ENV: &str = "NOTCHLING_RENDER_PAGE";

pub const FILE_ENV: &str = "NOTCHLING_RENDER_FILE";

pub const HTML_ENV: &str = "NOTCHLING_RENDER_HTML";

const HEADER: f32 = 36.0;

struct PanelEdge;

pub struct Shown {
    pub opens: Opens,
    pub title: String,
    pub subtitle: String,
    pub link: String,
    pub error: Option<String>,
    pub file: Option<FileState>,
    pub diff: Option<DiffState>,
    pub html: Option<HtmlView>,
}

pub struct FileState {
    pub path: PathBuf,
    pub loaded: Loaded,
    image: Option<Arc<gpui::Image>>,
}

impl FileState {
    pub(super) fn read(path: &Path) -> FileState {
        let loaded = file_panel::load(path);
        let image = match &loaded.body {
            Body::Image { format, bytes } => Some(Arc::new(gpui::Image::from_bytes(
                match format {
                    Pic::Png => ImageFormat::Png,
                    Pic::Jpeg => ImageFormat::Jpeg,
                    Pic::Gif => ImageFormat::Gif,
                    Pic::Webp => ImageFormat::Webp,
                    Pic::Bmp => ImageFormat::Bmp,
                    Pic::Tiff => ImageFormat::Tiff,
                    Pic::Svg => ImageFormat::Svg,
                },
                bytes.clone(),
            ))),
            _ => None,
        };
        FileState {
            path: path.to_path_buf(),
            loaded,
            image,
        }
    }
}

pub struct State {
    pub shown: Option<Shown>,
    pub width: f32,
    pub(super) web: Rc<RefCell<Option<PanelWeb>>>,
    pub render_only: bool,
    pub(super) asked: u64,
    drawn_width: Cell<f32>,
}

impl Default for State {
    fn default() -> State {
        State {
            shown: None,
            width: DEFAULT_WIDTH,
            web: Rc::new(RefCell::new(None)),
            render_only: false,
            asked: 0,
            drawn_width: Cell::new(DEFAULT_WIDTH),
        }
    }
}

impl State {
    pub fn preview(&mut self, id: &str) {
        let pages = crate::pages::renders::pages();
        let opens = match pages.iter().find(|p| p.id == id || p.folder == id) {
            Some(page) => Opens::Saved(Box::new(page.clone())),
            None => Opens::Live(ArtifactLink {
                id: id.to_owned(),
                code: false,
            }),
        };
        self.render_only = true;
        self.shown = Some(shown(opens));
    }

    pub fn preview_file(&mut self, path: &str) {
        self.render_only = true;
        let path = PathBuf::from(path);
        let opens = match file_panel::opens(&path) {
            opens @ Opens::Html(_) => opens,
            _ => Opens::File(path),
        };
        self.shown = Some(shown(opens));
    }

    pub fn attach_html(&mut self, html: HtmlView) {
        if let Some(shown) = self.shown.as_mut() {
            shown.html = Some(html);
        }
    }

    pub fn is_open(&self) -> bool {
        self.shown.is_some()
    }

    pub fn asked(&self) -> u64 {
        self.asked
    }

    pub fn take_keys_back(&self) {
        if let Some(web) = self.web.borrow().as_ref() {
            web.take_keys_back();
        }
    }

    pub fn shown_width(&self) -> f32 {
        self.drawn_width.get()
    }

    pub(super) fn set_shown_width(&self, width: f32) {
        self.drawn_width.set(width);
    }
}

fn shown(opens: Opens) -> Shown {
    let (title, link) = match &opens {
        Opens::Saved(page) => (page.title.clone(), page.link.clone()),
        Opens::Live(link) => ("Artifact".to_owned(), link.live_url()),
        Opens::File(path) | Opens::Html(path) => (file_panel::title(path), String::new()),
        Opens::Diff(_) | Opens::Folder(_) | Opens::System(_) | Opens::Browser => {
            (String::new(), String::new())
        }
    };
    let file = match &opens {
        Opens::File(path) => Some(FileState::read(path)),
        _ => None,
    };
    let html = match &opens {
        Opens::Html(path) => Some(HtmlView::file(path.clone(), home().as_deref())),
        _ => None,
    };
    let subtitle = match (&opens, &file, &html) {
        (Opens::File(path), Some(f), _) => {
            file_panel::subtitle(path, home().as_deref(), &f.loaded.body)
        }
        (_, _, Some(h)) => h.page_subtitle.clone(),
        _ => subtitle(&opens),
    };
    Shown {
        subtitle,
        opens,
        title,
        link,
        error: None,
        file,
        diff: None,
        html,
    }
}

pub(super) fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn follow_link(url: &str, window: &mut Window, cx: &mut App) {
    match window.root::<ChatView>() {
        Some(Some(chat)) => chat.update(cx, |view, cx| view.open_link(url.to_owned(), window, cx)),
        _ => cx.open_url(url),
    }
}

pub fn run_in_chat(command: &str, how: RunHow, window: &mut Window, cx: &mut App) {
    if let Some(Some(chat)) = window.root::<ChatView>() {
        chat.update(cx, |view, cx| view.run_command(command, how, cx));
    }
}

impl ChatView {
    pub(super) fn open_link(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self
            .selected_session()
            .map(|s| s.agent.cwd.clone())
            .unwrap_or_default();
        let whole = Path::new(&url);
        if whole.is_absolute() && whole.exists() {
            self.open_path(whole.to_path_buf(), window, cx);
            return;
        }
        if let Some(path) = crate::chat::drops::local_link(&url, &folder) {
            self.open_path(path, window, cx);
            return;
        }
        if url.starts_with('/') {
            return;
        }
        if artifact_link(&url).is_none() {
            if url.contains("://") || url.starts_with("mailto:") {
                cx.open_url(&url);
            }
            return;
        }
        self.page.asked += 1;
        let asked = self.page.asked;
        let library = app_library(cx);
        cx.spawn_in(window, async move |this, cx| {
            let pages = cx
                .background_executor()
                .spawn(async move { library.pages().map(|l| l.pages).unwrap_or_default() })
                .await;
            this.update_in(cx, |view, window, cx| {
                if view.page.asked == asked {
                    view.show_page(decide(&url, &pages), &url, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.show_page(file_panel::opens(&path), "", window, cx);
    }

    fn show_page(&mut self, opens: Opens, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        match &opens {
            Opens::Browser => {
                cx.open_url(url);
                return;
            }
            Opens::Folder(path) | Opens::System(path) => {
                cx.open_with_system(path);
                return;
            }
            _ => {}
        }
        self.page.web.borrow_mut().take();
        let mut page = shown(opens);
        if !self.page.render_only
            && matches!(
                page.opens,
                Opens::Saved(_) | Opens::Live(_) | Opens::Html(_)
            )
        {
            match self.make_web(&page.opens, window, cx) {
                Ok(web) => *self.page.web.borrow_mut() = Some(web),
                Err(e) => page.error = Some(e),
            }
        }
        self.page.shown = Some(page);
        cx.notify();
    }

    pub(super) fn refresh_file(&mut self, cx: &mut Context<Self>) {
        if let Some(Shown {
            file: Some(file), ..
        }) = self.page.shown.as_mut()
        {
            if file_panel::stamp(&file.path) != file.loaded.stamp {
                *file = FileState::read(&file.path);
                cx.notify();
            }
        }
        self.refresh_html(cx);
    }

    pub(super) fn web_hooks(&self, cx: &mut Context<Self>) -> PanelHooks {
        let asked = self.page.asked;
        let weak = cx.weak_entity();
        let titles = cx.to_async();
        let links = cx.to_async();
        PanelHooks {
            title: Box::new(move |title| {
                let weak = weak.clone();
                titles
                    .spawn(async move |cx| {
                        weak.update(cx, |view, cx| {
                            let live = matches!(
                                view.page.shown.as_ref().map(|s| &s.opens),
                                Some(Opens::Live(_))
                            );
                            if let (true, true, Some(shown)) =
                                (live, view.page.asked == asked, view.page.shown.as_mut())
                            {
                                shown.title = title;
                                cx.notify();
                            }
                        })
                        .ok();
                    })
                    .detach();
            }),
            leave: Box::new(move |url| {
                links
                    .spawn(async move |cx| {
                        cx.update(|cx| cx.open_url(&url)).ok();
                    })
                    .detach();
            }),
        }
    }

    fn make_web(
        &self,
        opens: &Opens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<PanelWeb, String> {
        let hooks = self.web_hooks(cx);
        match opens {
            Opens::Html(path) => self.make_html_web(path, window, cx),
            Opens::Saved(page) => {
                let pages_dir = cx
                    .try_global::<AppPagesDir>()
                    .map(|d| d.0.clone())
                    .ok_or("There is no pages folder here.")?;
                let path = local::inside(&pages_dir, Path::new(&page.path))?;
                let source = local::source_for(&path)?;
                PanelWeb::open(window, PanelPage::Local(&source), hooks)
            }
            Opens::Live(link) => PanelWeb::open(window, PanelPage::Live(&link.live_url()), hooks),
            _ => Err("Not a page.".to_owned()),
        }
    }

    pub(super) fn close_page(&mut self, cx: &mut Context<Self>) {
        self.page.asked += 1;
        self.page.shown = None;
        self.page.web.borrow_mut().take();
        super::select::clear();
        self.details_hidden = false;
        self.git_ask(crate::chat::git::Why::Opened, cx);
        cx.notify();
    }

    fn drag_edge(&mut self, event: &DragMoveEvent<PanelEdge>, cx: &mut Context<Self>) {
        let area = event.bounds;
        self.page.width = dragged_width(
            f32::from(event.event.position.x),
            f32::from(area.left()),
            f32::from(area.right()),
        );
        cx.notify();
    }
}

pub(super) fn resize_edge() -> gpui::Stateful<gpui::Div> {
    div()
        .id("page-panel-edge")
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(-8.0))
        .w(px(9.0))
        .cursor(CursorStyle::ResizeLeftRight)
        .on_drag(PanelEdge, |_, _, _, cx| cx.new(|_| EmptyView))
}

pub fn follows_edge<E: InteractiveElement>(element: E, cx: &mut Context<ChatView>) -> E {
    element.on_drag_move::<PanelEdge>(cx.listener(|this, event, _, cx| this.drag_edge(event, cx)))
}

fn web_page(view: &ChatView, shown: &Shown, a: Appearance) -> AnyElement {
    let note = match (&shown.error, &shown.opens) {
        (Some(error), _) => error.clone(),
        (None, Opens::Live(_)) => "Opening the page on claude.ai…".to_owned(),
        (None, _) if shown.html.is_some() => "Opening the page…".to_owned(),
        (None, _) => "Opening the saved copy…".to_owned(),
    };
    let ink = if shown.error.is_some() {
        Ink::Expired
    } else {
        Ink::TextMuted
    };
    let web = view.page.web.clone();
    let visible = !view.page_covered && shown.error.is_none();
    if !visible {
        if let Some(web) = web.borrow().as_ref() {
            web.hide();
        }
    }
    div()
        .relative()
        .flex_1()
        .min_h(px(0.0))
        .flex()
        .items_center()
        .justify_center()
        .p(px(24.0))
        .child(label(&note, TypeRole::Meta, ink, a))
        .child(
            canvas(
                |bounds, _, _| bounds,
                move |_, bounds, window, _| {
                    if let Some(web) = web.borrow().as_ref() {
                        let frame = Frame {
                            x: f32::from(bounds.origin.x).into(),
                            y: f32::from(bounds.origin.y).into(),
                            width: f32::from(bounds.size.width).into(),
                            height: f32::from(bounds.size.height).into(),
                        };
                        web.place(frame, visible, window.scale_factor().into());
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .into_any_element()
}

const FILE_PAD: f32 = 16.0;
const CHUNK_LINES: usize = 80;

fn file_body(
    file: &FileState,
    width: f32,
    markdown: &RefCell<MarkdownCache>,
    chat: WeakEntity<ChatView>,
    a: Appearance,
    cx: &App,
) -> AnyElement {
    let key = format!("file-panel-{}", file.path.display());
    let note = |text: &str, ink: Ink| label(text, TypeRole::Meta, ink, a).into_any_element();
    let mut cut = false;
    let content: AnyElement = match &file.loaded.body {
        Body::Markdown { text, cut: c } => {
            cut = *c;
            let folder = file_panel::folder_of(&file.path);
            let message = markdown
                .borrow_mut()
                .message(&key, text, &folder.to_string_lossy());
            super::markdown::render(&key, &super::markdown::Sel::File, &message, a, cx)
                .into_any_element()
        }
        Body::Text { text, cut: c } => {
            cut = *c;
            if text.is_empty() {
                note("This file is empty.", Ink::TextMuted)
            } else {
                let lines: Vec<&str> = text.split_inclusive('\n').collect();
                div()
                    .w_full()
                    .kit_type(TypeRole::Mono)
                    .ink(Ink::Text, a)
                    .flex()
                    .flex_col()
                    .children(lines.chunks(CHUNK_LINES).enumerate().map(|(i, chunk)| {
                        super::markdown::plain(
                            &format!("{key}-t{i}"),
                            &super::markdown::Sel::File,
                            chunk.concat().trim_end(),
                            a,
                        )
                    }))
                    .into_any_element()
            }
        }
        Body::Image { bytes, .. } => match &file.image {
            Some(image) => {
                let (w, h) = crate::chat::image_size::size(&bytes[..bytes.len().min(64 * 1024)])
                    .unwrap_or((4, 3));
                let room = (width - 2.0 * FILE_PAD).max(40.0);
                let shown_w = room.min(w.max(1) as f32);
                let shown_h = shown_w * h as f32 / w.max(1) as f32;
                div()
                    .w(px(shown_w))
                    .h(px(shown_h))
                    .rounded(px(RADIUS_MD))
                    .border(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
                    .overflow_hidden()
                    .child(
                        img(image.clone())
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                    )
                    .into_any_element()
            }
            None => note("Can't show this picture.", Ink::TextMuted),
        },
        Body::Other => {
            let path = file.path.clone();
            div()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(10.0))
                .child(note("Can't show this file here.", Ink::TextMuted))
                .child(
                    button(
                        "file-panel-open-default",
                        "Open in Default App",
                        ButtonKind::Secondary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(move |_, _, cx| cx.open_with_system(&path)),
                )
                .into_any_element()
        }
        Body::Missing(why) => note(why, Ink::Expired),
    };
    div()
        .id(SharedString::from(format!("{key}-scroll")))
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .child(
            div()
                .w_full()
                .p(px(FILE_PAD))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(super::select::listener(chat))
                .child(content)
                .when(cut, |d| {
                    d.child(label(
                        "Only the first 1 MB is shown. Open it in its app to read the rest.",
                        TypeRole::Meta,
                        Ink::TextMuted,
                        a,
                    ))
                }),
        )
        .into_any_element()
}

pub fn render(
    view: &ChatView,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Option<AnyElement> {
    let shown = view.page.shown.as_ref()?;
    let sidebar = view.sidebar_width();
    let available = f32::from(window.viewport_size().width) - sidebar;
    let width = panel_width(view.page.width, available);
    view.page.drawn_width.set(width);

    let html_page = shown.html.as_ref().is_some_and(HtmlView::is_page);
    let file_path = shown
        .file
        .as_ref()
        .map(|f| f.path.clone())
        .or_else(|| shown.html.as_ref().map(|h| h.path.clone()));
    let is_diff = shown.diff.is_some();
    let is_file = file_path.is_some() || is_diff;
    let subtitle = match &shown.html {
        Some(html) if html.is_page() => html.page_subtitle.clone(),
        _ => shown.subtitle.clone(),
    };

    let mark = gate::tip_text(
        div().id("page-panel-mark").flex_shrink_0().child(icon(
            if is_file { Icon::File } else { Icon::Page },
            12.0,
            Ink::AccentText,
            a,
        )),
        subtitle.into(),
        a,
    );
    let title = label(&shown.title, TypeRole::Label, Ink::Text, a)
        .flex_1()
        .min_w(px(0.0))
        .elide();
    let tool = |id: &'static str, shape: Icon, tip: &'static str| {
        gate::tip_text(
            icon_button(
                id,
                shape,
                IconSize::Button.points(),
                ButtonKind::Ghost,
                PartState::Rest,
                a,
            ),
            tip.into(),
            a,
        )
        .flex_shrink_0()
    };
    let web_for_reload = view.page.web.clone();
    let refresh = (!is_file || html_page).then(|| {
        tool("page-panel-refresh", Icon::Resume, "Reload the page").on_click(move |_, _, _| {
            if let Some(web) = web_for_reload.borrow().as_ref() {
                web.reload();
            }
        })
    });
    let browser = (!shown.link.is_empty()).then(|| {
        let link = shown.link.clone();
        tool("page-panel-browser", Icon::Web, "Open in Browser")
            .on_click(move |_, _, cx| cx.open_url(&link))
    });
    let reveal = file_path.clone().map(|path| {
        tool("page-panel-reveal", Icon::Folder, "Show in Finder")
            .on_click(move |_, _, cx| cx.reveal_path(&path))
    });
    let open_with = file_path.clone().map(|path| {
        tool("page-panel-open", Icon::Expand, "Open in Default App")
            .on_click(move |_, _, cx| cx.open_with_system(&path))
    });
    let looks = shown
        .html
        .as_ref()
        .map(|html| super::html_panel::looks(html, a, cx));
    let back = tool(
        "page-panel-back",
        Icon::Close,
        if is_file { "Close" } else { "Back to Details" },
    )
    .on_click(cx.listener(|this, _, _, cx| this.close_page(cx)));
    let header = div()
        .h(px(HEADER))
        .flex_shrink_0()
        .pl(px(12.0))
        .pr(px(6.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(mark)
        .child(title)
        .children(looks)
        .children(refresh)
        .children(browser)
        .children(reveal)
        .children(open_with)
        .child(back);

    let page = match (shown.file.as_ref(), shown.diff.as_ref()) {
        _ if html_page => web_page(view, shown, a),
        (_, Some(diff)) => diff_panel::body(diff, cx.entity().downgrade(), a),
        (Some(file), None) => file_body(
            file,
            width,
            &view.parts.conversation.read(cx).s.markdown,
            cx.entity().downgrade(),
            a,
            cx,
        ),
        (None, None) => web_page(view, shown, a),
    };

    Some(
        div()
            .relative()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .fill(Ink::Window, a)
            .border_l(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .flex()
            .flex_col()
            .child(header)
            .child(div().h(px(1.0)).flex_shrink_0().fill(Ink::Hairline, a))
            .child(page)
            .child(resize_edge())
            .window_region(a, cx)
            .part_at(
                if is_file { "File Panel" } else { "Page Panel" },
                Corner::TopTrailing,
                a,
                cx,
            )
            .into_any_element(),
    )
}
