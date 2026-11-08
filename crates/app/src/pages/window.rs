//! The Artifacts window (part U4, macOS): one window over the pages library, opened from the search
//! panel (Return on a page) and brought forward when it is already open; and the page windows it
//! opens, each a `wry` web view over the local copy (`web`). A titled window, so it counts toward
//! the Dock icon like Chat (`mac::shell::note_titled`).

use std::path::Path;
use std::sync::Arc;

use gpui::{
    prelude::*, px, size, App, Bounds, Global, TitlebarOptions, WindowBounds, WindowHandle,
    WindowKind, WindowOptions,
};

use super::local;
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
    open: Vec<PageWindow>,
}

impl Global for PagesWindows {}

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

/// Opens `page`'s local copy in a window of its own. The line to show when it can't.
pub fn show_page(cx: &mut App, page: &Page) -> Result<(), String> {
    let source = local::source_for(Path::new(&page.path))?;
    let windows = cx.default_global::<PagesWindows>();
    // Windows the person closed are let go of here, where a new one is about to open.
    windows.open.retain(PageWindow::is_open);
    let window = PageWindow::open(&page.title, &source, windows.open.len())?;
    windows.open.push(window);
    Ok(())
}
