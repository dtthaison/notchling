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
use std::path::Path;
use std::rc::Rc;

use gpui::{
    canvas, div, prelude::*, px, AnyElement, App, Context, CursorStyle, DragMoveEvent, EmptyView,
    Window,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SIDEBAR_WIDTH;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{hsla, icon, icon_button, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::ChatView;
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
        Opens::Browser => (String::new(), String::new()),
    };
    Shown {
        subtitle: subtitle(&opens),
        opens,
        title,
        link,
        error: None,
    }
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
        // A file on this Mac (a picture's tile, an image under a reply, a path relative to the
        // session's folder): macOS opens it in its app.
        let folder = self
            .selected_session()
            .map(|s| s.agent.cwd.clone())
            .unwrap_or_default();
        if let Some(path) = crate::chat::drops::local_link(&url, &folder) {
            cx.open_url(&crate::chat::drops::file_url(&path));
            return;
        }
        if url.starts_with('/') {
            cx.open_url(&crate::chat::drops::file_url(std::path::Path::new(&url)));
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

    /// Shows what a click opens: the page in the panel, in place of the one there.
    fn show_page(&mut self, opens: Opens, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        if opens == Opens::Browser {
            cx.open_url(url);
            return;
        }
        self.page.web.borrow_mut().take();
        let mut page = shown(opens);
        if !self.page.render_only {
            match self.make_web(&page.opens, window, cx) {
                Ok(web) => *self.page.web.borrow_mut() = Some(web),
                Err(e) => page.error = Some(e),
            }
        }
        self.page.shown = Some(page);
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
            Opens::Browser => Err("Not a page.".to_owned()),
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

    // One line: the Artifact mark (where the page comes from on hover), the title in all the room
    // the three icon buttons leave, then Refresh, Open in Browser and Back to Details.
    let mark = gate::tip_text(
        div().id("page-panel-mark").flex_shrink_0().child(icon(
            Icon::Page,
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
    let refresh =
        tool("page-panel-refresh", Icon::Resume, "Reload the page").on_click(move |_, _, _| {
            if let Some(web) = web_for_reload.borrow().as_ref() {
                web.reload();
            }
        });
    let browser = (!shown.link.is_empty()).then(|| {
        let link = shown.link.clone();
        tool("page-panel-browser", Icon::Web, "Open in Browser")
            .on_click(move |_, _, cx| cx.open_url(&link))
    });
    let back = tool("page-panel-back", Icon::Close, "Back to Details")
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
        .child(refresh)
        .children(browser)
        .child(back);

    // Under the web view: what shows while it loads, while an overlay hides it, and in a render.
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
    let page = div()
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
        );

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
            .part_at("Page Panel", Corner::TopTrailing, a, cx)
            .into_any_element(),
    )
}
