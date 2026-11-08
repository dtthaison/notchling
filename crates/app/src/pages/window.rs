//! The Artifacts window (part U4, macOS): one window over the pages library, opened from the search
//! panel (Return on a page) and brought forward when it is already open; and the page windows it
//! opens, each a `wry` web view over the local copy (`web`). A titled window, so it counts toward
//! the Dock icon like Chat (`mac::shell::note_titled`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    prelude::*, px, size, App, Bounds, Global, TitlebarOptions, WindowBounds, WindowHandle,
    WindowKind, WindowOptions,
};

use super::local;
use super::set::PageSet;
use super::view::PagesView;
use super::web::PageWindow;
use crate::search::library::Library;
use crate::search::panel::app_library;
use crate::search::wire::Page;

/// The window's size when it opens, and the least it can be.
pub const SIZE: (f32, f32) = (1080.0, 720.0);
pub const MIN_SIZE: (f32, f32) = (760.0, 480.0);

/// The Artifacts window while it is open, and the page windows opened from it.
#[derive(Default)]
pub struct PagesWindows {
    window: Option<WindowHandle<PagesView>>,
    open: PageSet<PageWindow>,
}

impl Global for PagesWindows {}

/// The library's folder: the only place a page's copy is loaded from. Not set in a render.
pub struct AppPagesDir(pub PathBuf);

impl Global for AppPagesDir {}

/// What is said when the app has no pages folder to check a copy against.
const NO_PAGES_DIR: &str = "There is no pages folder here.";

/// Opens the Artifacts window, or brings it forward, and selects `select`'s page when given.
pub fn open(cx: &mut App, select: Option<String>) {
    let existing = cx.default_global::<PagesWindows>().window;
    if let Some(handle) = existing {
        let brought = handle.update(cx, |view, window, cx| {
            if let Some(key) = &select {
                view.select(key, cx);
            }
            window.activate_window();
        });
        if brought.is_ok() {
            cx.activate(true);
            return;
        }
    }
    let library: Arc<Library> = app_library(cx);
    let bounds = Bounds::centered(None, size(px(SIZE.0), px(SIZE.1)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Artifacts".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        window_min_size: Some(size(px(MIN_SIZE.0), px(MIN_SIZE.1))),
        kind: WindowKind::Normal,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        cx.new(|cx| PagesView::live(library, select, window, cx))
    });
    match opened {
        Ok(handle) => {
            cx.default_global::<PagesWindows>().window = Some(handle);
            crate::mac::shell::note_titled(cx, handle.window_id());
            cx.activate(true);
        }
        Err(e) => eprintln!("open the Artifacts window: {e}"),
    }
}

/// Opens `page`'s local copy in a window of its own. The line to show when it can't. The copy has
/// to be in the library's folder: the daemon names the path, and the page is given read access to
/// the folder it is in.
pub fn show_page(cx: &mut App, page: &Page) -> Result<(), String> {
    let pages_dir = cx
        .try_global::<AppPagesDir>()
        .map(|d| d.0.clone())
        .ok_or(NO_PAGES_DIR)?;
    let path = local::inside(&pages_dir, Path::new(&page.path))?;
    let source = local::source_for(&path)?;
    let async_cx = cx.to_async();
    let windows = cx.default_global::<PagesWindows>();
    // Windows the person closed without a notice reaching us are let go of here too.
    windows.open.retain_open(PageWindow::is_open);
    let window = PageWindow::open(&page.title, &source, windows.open.len(), move |id| {
        // Called while AppKit closes the window, when the app may be mid-update: let go of the
        // window a moment later, from the app's own queue.
        async_cx
            .spawn(async move |cx| {
                cx.update(|cx| {
                    cx.default_global::<PagesWindows>().open.forget(id);
                })
                .ok();
            })
            .detach();
    })?;
    windows.open.push(window.id(), window);
    Ok(())
}
