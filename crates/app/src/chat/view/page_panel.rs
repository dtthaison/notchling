//! The page panel (the Swift `PagePanel`): an Artifact clicked in a conversation, in the Details
//! Panel's place beside the chat. A header (the page's title, where it comes from, Open in Browser,
//! Back to Details) over the page, which is a `wry` web view (`pages::web::PanelWeb`) laid over the
//! panel's page area: a canvas there reports its bounds each frame and the web view's frame follows
//! them. The web view hides while a sheet, menu or the part names are up, since GPUI draws those
//! under it, and goes when the page is closed or the window is (the view drops it).
//!
//! The panel belongs to the window, not to a session: picking another session keeps it, as the
//! Swift panel sat outside the per-session identity. Its left edge drags to resize it.
//! `chat::page_panel` decides what a click opens and holds the panel's pure rules.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    canvas, div, img, prelude::*, px, AnyElement, App, Context, CursorStyle, DragMoveEvent,
    EmptyView, ImageFormat, ObjectFit, SharedString, Window,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{RADIUS_MD, SIDEBAR_WIDTH};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, icon, icon_button, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::ChatView;
use crate::chat::file_panel::{self, Body, Loaded, Pic};
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

/// Set to a page's id or folder, a render draws the page panel over it: a page the made-up library
/// (`pages::renders`) has as its saved copy, any other id as a live page. The panel's chrome only:
/// the web view is an AppKit view a render doesn't make.
pub const PAGE_ENV: &str = "NOTCHLING_RENDER_PAGE";

/// Set to a file's path, a render draws the file panel over it.
pub const FILE_ENV: &str = "NOTCHLING_RENDER_FILE";

/// What the header's height is (the Swift panel's 48).
const HEADER: f32 = 36.0;

/// The panel's left edge, dragged.
struct PanelEdge;

/// The page shown, as the header names it.
pub struct Shown {
    pub opens: Opens,
    pub title: String,
    pub subtitle: String,
    /// What Open in Browser opens; "" when there is nothing on claude.ai to open.
    pub link: String,
    /// Why the page can't show (its copy is gone or outside the library, the web view failed).
    pub error: Option<String>,
    /// The file shown, for [`Opens::File`].
    pub file: Option<FileState>,
}

/// A file as last read for the panel.
pub struct FileState {
    pub path: PathBuf,
    pub loaded: Loaded,
    /// A picture's bytes made once for GPUI, which draws from them.
    image: Option<Arc<gpui::Image>>,
}

impl FileState {
    fn read(path: &Path) -> FileState {
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
    /// The width asked for, by default or by dragging the edge; shown within `panel_width`.
    pub width: f32,
    /// The web view while a page is open in a live window. Shared with the frame's canvas, which
    /// lays it out.
    web: Rc<RefCell<Option<PanelWeb>>>,
    /// A render draws the chrome alone.
    pub render_only: bool,
    /// Counts the clicks, so the library's answer to an older one is dropped.
    asked: u64,
    /// The width last drawn, for what is placed beside the panel (the account menu).
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
    /// A render's panel over `id` (`NOTCHLING_RENDER_PAGE`).
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

    /// A render's panel over the file at `path` (`NOTCHLING_RENDER_FILE`).
    pub fn preview_file(&mut self, path: &str) {
        self.render_only = true;
        self.shown = Some(shown(Opens::File(PathBuf::from(path))));
    }

    pub fn is_open(&self) -> bool {
        self.shown.is_some()
    }

    /// Gives the keyboard back to Chat if the page has it.
    pub fn take_keys_back(&self) {
        if let Some(web) = self.web.borrow().as_ref() {
            web.take_keys_back();
        }
    }

    /// The panel's width as last drawn.
    pub fn shown_width(&self) -> f32 {
        self.drawn_width.get()
    }
}

/// The header's words and link for what a click opened.
fn shown(opens: Opens) -> Shown {
    let (title, link) = match &opens {
        Opens::Saved(page) => (page.title.clone(), page.link.clone()),
        Opens::Live(link) => ("Artifact".to_owned(), link.live_url()),
        Opens::File(path) => (file_panel::title(path), String::new()),
        Opens::Folder(_) | Opens::System(_) | Opens::Browser => (String::new(), String::new()),
    };
    let file = match &opens {
        Opens::File(path) => Some(FileState::read(path)),
        _ => None,
    };
    let subtitle = match (&opens, &file) {
        (Opens::File(path), Some(f)) => {
            file_panel::subtitle(path, home().as_deref(), &f.loaded.body)
        }
        _ => subtitle(&opens),
    };
    Shown {
        subtitle,
        opens,
        title,
        link,
        error: None,
        file,
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// A link clicked in a reply (`markdown`): in the Chat window, the window's view decides; anywhere
/// else, the browser.
pub fn follow_link(url: &str, window: &mut Window, cx: &mut App) {
    match window.root::<ChatView>() {
        Some(Some(chat)) => chat.update(cx, |view, cx| view.open_link(url.to_owned(), window, cx)),
        _ => cx.open_url(url),
    }
}

impl ChatView {
    /// A clicked link: an Artifact opens in the page panel (its saved copy when the library has
    /// one, else the live page), anything else in the browser. The library is asked off the UI
    /// thread; without a daemon it has no pages, so an Artifact opens live.
    pub(super) fn open_link(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        // A file or folder on this Mac (a path in a reply, a picture's tile, a path relative to the
        // session's folder): a file shows beside the chat, a folder opens in Finder.
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
            // A path that isn't there has nothing to open; macOS would only say -50.
            return;
        }
        if artifact_link(&url).is_none() {
            // A path that isn't there has nothing to open; macOS would only say -50.
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

    /// A file or folder a click named: a file the panel can show goes in it, a folder opens in
    /// Finder and any other file in its own app.
    pub(super) fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.show_page(file_panel::opens(&path), "", window, cx);
    }

    /// Shows what a click opens: the page or file in the panel, in place of the one there.
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
        if !self.page.render_only && matches!(page.opens, Opens::Saved(_) | Opens::Live(_)) {
            match self.make_web(&page.opens, window, cx) {
                Ok(web) => *self.page.web.borrow_mut() = Some(web),
                Err(e) => page.error = Some(e),
            }
        }
        self.page.shown = Some(page);
        cx.notify();
    }

    /// Reads the open file again when it has changed on disk (looked at on every tick).
    pub(super) fn refresh_file(&mut self, cx: &mut Context<Self>) {
        let Some(Shown {
            file: Some(file), ..
        }) = self.page.shown.as_mut()
        else {
            return;
        };
        if file_panel::stamp(&file.path) == file.loaded.stamp {
            return;
        }
        *file = FileState::read(&file.path);
        cx.notify();
    }

    /// The web view for `opens`: a saved page's copy, checked to be in the library as a page
    /// window's is, or the live page.
    fn make_web(
        &self,
        opens: &Opens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<PanelWeb, String> {
        let asked = self.page.asked;
        let weak = cx.weak_entity();
        let titles = cx.to_async();
        let links = cx.to_async();
        // Both are called from WebKit, maybe mid-update: act a moment later, on the app's queue.
        let hooks = PanelHooks {
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
        };
        match opens {
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

    /// Back to Details: the page goes, and the Details Panel shows in its place.
    pub(super) fn close_page(&mut self, cx: &mut Context<Self>) {
        self.page.asked += 1;
        self.page.shown = None;
        self.page.web.borrow_mut().take();
        self.details_hidden = false;
        cx.notify();
    }

    /// The panel's edge dragged over the area it shares with the conversation.
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

/// Lets the row holding the conversation and the panel follow the panel's edge while it is
/// dragged.
pub fn follows_edge<E: InteractiveElement>(element: E, cx: &mut Context<ChatView>) -> E {
    element.on_drag_move::<PanelEdge>(cx.listener(|this, event, _, cx| this.drag_edge(event, cx)))
}

/// Under the web view: what shows while it loads, while an overlay hides it, and in a render.
/// The canvas over it carries the web view's frame.
fn web_page(view: &ChatView, shown: &Shown, a: Appearance) -> AnyElement {
    let note = match (&shown.error, &shown.opens) {
        (Some(error), _) => error.clone(),
        (None, Opens::Live(_)) => "Opening the page on claude.ai…".to_owned(),
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

/// Room around a file's content.
const FILE_PAD: f32 = 16.0;
/// Lines of a text file drawn as one element: each is laid out once, so a long file isn't one
/// huge text.
const CHUNK_LINES: usize = 80;

/// A file's content, scrolling below the header: Markdown as a message is drawn, text in the mono
/// face, a picture at the panel's width, and for anything else a note and a button to its own app.
fn file_body(
    file: &FileState,
    width: f32,
    markdown: &RefCell<MarkdownCache>,
    a: Appearance,
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
            super::markdown::render(&key, &message, a).into_any_element()
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
                        super::markdown::plain(&format!("{key}-t{i}"), chunk.concat().trim_end(), a)
                    }))
                    .into_any_element()
            }
        }
        Body::Image { bytes, .. } => match &file.image {
            Some(image) => {
                let (w, h) = crate::chat::image_size::size(&bytes[..bytes.len().min(64 * 1024)])
                    .unwrap_or((4, 3));
                let room = (width - 2.0 * FILE_PAD).max(40.0);
                // Never drawn larger than it is; GPUI sizes an image's box from its pixels, so
                // the height follows the shape.
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

/// The panel, while a page is open.
pub fn render(
    view: &ChatView,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Option<AnyElement> {
    let shown = view.page.shown.as_ref()?;
    let sidebar = if view.sidebar_hidden {
        0.0
    } else {
        SIDEBAR_WIDTH
    };
    let available = f32::from(window.viewport_size().width) - sidebar;
    let width = panel_width(view.page.width, available);
    view.page.drawn_width.set(width);

    let file_path = shown.file.as_ref().map(|f| f.path.clone());
    let is_file = file_path.is_some();

    // One line: the Artifact mark (where the page comes from on hover), the title in all the room
    // the three icon buttons leave, then Refresh, Open in Browser and Back to Details.
    let mark = gate::tip_text(
        div().id("page-panel-mark").flex_shrink_0().child(icon(
            if file_path.is_some() {
                Icon::File
            } else {
                Icon::Page
            },
            12.0,
            Ink::AccentText,
            a,
        )),
        shown.subtitle.clone().into(),
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
    let refresh = file_path.is_none().then(|| {
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
    // A file: where it is, and its own app.
    let reveal = file_path.clone().map(|path| {
        tool("page-panel-reveal", Icon::Folder, "Show in Finder")
            .on_click(move |_, _, cx| cx.reveal_path(&path))
    });
    let open_with = file_path.clone().map(|path| {
        tool("page-panel-open", Icon::Expand, "Open in Default App")
            .on_click(move |_, _, cx| cx.open_with_system(&path))
    });
    let back = tool(
        "page-panel-back",
        Icon::Close,
        if file_path.is_some() {
            "Close"
        } else {
            "Back to Details"
        },
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
        .children(refresh)
        .children(browser)
        .children(reveal)
        .children(open_with)
        .child(back);

    // A file: its text, in the room below the header.
    let page = match shown.file.as_ref() {
        Some(file) => file_body(file, width, &view.markdown, a),
        None => web_page(view, shown, a),
    };

    // The edge to drag: over the hairline and just left of it, never over the web view, which
    // would take the press.
    let edge = div()
        .id("page-panel-edge")
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(-8.0))
        .w(px(9.0))
        .cursor(CursorStyle::ResizeLeftRight)
        .on_drag(PanelEdge, |_, _, _, cx| cx.new(|_| EmptyView));

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
            .child(edge)
            .part_at(
                if is_file { "File Panel" } else { "Page Panel" },
                Corner::TopTrailing,
                a,
                cx,
            )
            .into_any_element(),
    )
}
