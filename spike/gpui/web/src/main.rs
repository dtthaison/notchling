//! S2: a GPUI window that opens pages in wry web views, each in its own NSWindow,
//! in the same process and run loop.

use std::ptr::NonNull;

use gpui::{
    div, point, prelude::*, px, size, App, Application, Bounds, Context, FocusHandle,
    KeyDownEvent, MouseButton, WindowBounds, WindowOptions,
};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBackingStoreType, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
};

/// A page window's content view, as the raw handle wry builds into.
struct ViewHandle(Retained<NSView>);

impl HasWindowHandle for ViewHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let ptr = NonNull::from(&*self.0).cast();
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(ptr));
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

struct Page {
    _window: Retained<NSWindow>,
    _view: wry::WebView,
}

fn open_page(url: String, n: usize) -> Result<Page, String> {
    let mtm = MainThreadMarker::new().ok_or("not on main thread")?;
    let rect = NSRect::new(NSPoint::new(200. + 40. * n as f64, 200. - 30. * n as f64), NSSize::new(800., 600.));
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable;
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // We own it; AppKit must not free it on close.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("Page"));
    let content = window.contentView().ok_or("no content view")?;
    let titled = window.clone();
    let view = wry::WebViewBuilder::new()
        .with_url(url)
        .with_document_title_changed_handler(move |t| {
            eprintln!("page title: {t}");
            titled.setTitle(&NSString::from_str(&t));
        })
        .build(&ViewHandle(content))
        .map_err(|e| e.to_string())?;
    window.makeKeyAndOrderFront(None);
    Ok(Page { _window: window, _view: view })
}

struct Main {
    focus: FocusHandle,
    pages: Vec<Page>,
    typed: String,
    status: String,
}

impl Render for Main {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                let k = e.keystroke.key_char.clone().unwrap_or_else(|| e.keystroke.key.clone());
                eprintln!("gpui key: {k}");
                this.typed.push_str(&k);
                cx.notify();
            }))
            .size_full()
            .bg(gpui::white())
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .px_3()
                    .py_1()
                    .bg(gpui::blue())
                    .text_color(gpui::white())
                    .rounded_md()
                    .child("Open Page")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            let n = this.pages.len();
                            let url = if n % 2 == 0 {
                                "https://example.com".to_string()
                            } else {
                                format!("file://{}/test.html", env!("CARGO_MANIFEST_DIR"))
                            };
                            this.status = match open_page(url.clone(), n) {
                                Ok(p) => {
                                    this.pages.push(p);
                                    format!("opened {url}")
                                }
                                Err(e) => format!("failed {url}: {e}"),
                            };
                            eprintln!("{}", this.status);
                            cx.notify();
                        }),
                    ),
            )
            .child(format!("pages: {} · {}", self.pages.len(), self.status))
            .child(format!("typed here: {}", self.typed))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds { origin: point(px(100.), px(100.)), size: size(px(480.), px(240.)) };
        let options = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
        cx.open_window(options, |window, cx| {
            cx.new(|cx| {
                let focus = cx.focus_handle();
                window.focus(&focus);
                Main { focus, pages: Vec::new(), typed: String::new(), status: String::new() }
            })
        })
        .expect("open window");
        cx.activate(true);
    });
}
