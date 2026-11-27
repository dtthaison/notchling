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

pub const SIZE: (f32, f32) = (1080.0, 720.0);
pub const MIN_SIZE: (f32, f32) = (760.0, 480.0);

#[derive(Default)]
pub struct PagesWindows {
    window: Option<WindowHandle<PagesView>>,
    open: PageSet<PageWindow>,
}

impl Global for PagesWindows {}

pub struct AppPagesDir(pub PathBuf);

impl Global for AppPagesDir {}

const NO_PAGES_DIR: &str = "There is no pages folder here.";

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

pub fn show_page(cx: &mut App, page: &Page) -> Result<(), String> {
    let pages_dir = cx
        .try_global::<AppPagesDir>()
        .map(|d| d.0.clone())
        .ok_or(NO_PAGES_DIR)?;
    let path = local::inside(&pages_dir, Path::new(&page.path))?;
    let source = local::source_for(&path)?;
    let async_cx = cx.to_async();
    let windows = cx.default_global::<PagesWindows>();
    windows.open.retain_open(PageWindow::is_open);
    let window = PageWindow::open(&page.title, &source, windows.open.len(), move |id| {
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
