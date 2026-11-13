use std::cell::Cell;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSBackingStoreType, NSView, NSWindow, NSWindowStyleMask, NSWindowWillCloseNotification,
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
    NSURL,
};
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{
    NewWindowResponse, Rect, WebView, WebViewBuilder, WebViewBuilderExtDarwin, WebViewExtMacOS,
};

use super::local::{stays_local, PageSource};
use super::set::counts_as_open;
use crate::chat::page_panel::{appkit_frame, live_allows, opens_in_browser, Frame};

pub const LIVE_STORE: [u8; 16] = *b"notchling-claude";

const SIZE: (f64, f64) = (960.0, 720.0);
const MIN_SIZE: (f64, f64) = (480.0, 360.0);
const STEP: f64 = 28.0;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct ViewHandle(Retained<NSView>);

impl HasWindowHandle for ViewHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let ptr = NonNull::from(&*self.0).cast();
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(ptr));
        // SAFETY: the view is retained by this handle for as long as the handle is borrowed.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

pub struct PageWindow {
    window: Retained<NSWindow>,
    id: u64,
    closing: Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    view: WebView,
}

impl PageWindow {
    pub fn open(
        title: &str,
        source: &PageSource,
        nth: usize,
        on_close: impl Fn(u64) + 'static,
    ) -> Result<PageWindow, String> {
        let mtm = MainThreadMarker::new().ok_or("a page window opens on the main thread")?;
        let offset = STEP * nth as f64;
        let rect = NSRect::new(
            NSPoint::new(120.0 + offset, 120.0 - offset),
            NSSize::new(SIZE.0, SIZE.1),
        );
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        // SAFETY: AppKit's designated initializer on a freshly allocated window, on the main thread.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: this struct owns the window; AppKit must not free it when the person closes it.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(title));
        window.setContentMinSize(NSSize::new(MIN_SIZE.0, MIN_SIZE.1));
        if nth == 0 {
            window.center();
        }
        let content = window
            .contentView()
            .ok_or("the page window has no content view")?;
        let titled = Weak::from_retained(&window);
        let view = local_builder(source)
            .with_document_title_changed_handler(move |t| {
                if let (false, Some(window)) = (t.is_empty(), titled.load()) {
                    window.setTitle(&NSString::from_str(&t));
                }
            })
            .build(&ViewHandle(content))
            .map_err(|e| format!("the web view couldn't be made: {e}"))?;
        load_local(&view, source);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let notice = RcBlock::new(move |_: NonNull<NSNotification>| on_close(id));
        let window_object: &AnyObject = window.as_ref();
        // SAFETY: with no queue the block runs on the thread that closes the window, the main
        // thread, where everything it touches lives; it is only ever called there.
        let closing = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSWindowWillCloseNotification),
                Some(window_object),
                None,
                &notice,
            )
        };
        window.makeKeyAndOrderFront(None);
        Ok(PageWindow {
            window,
            id,
            closing: Some(closing),
            view,
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn is_open(&self) -> bool {
        counts_as_open(self.window.isVisible(), self.window.isMiniaturized())
    }
}

impl Drop for PageWindow {
    fn drop(&mut self) {
        if let Some(registration) = self.closing.take() {
            // SAFETY: the registration is the one `open` made, removed once.
            let observer: &AnyObject = registration.as_ref();
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer) };
        }
        retire(&self.view);
        self.window.close();
    }
}

fn local_builder<'a>(source: &PageSource) -> WebViewBuilder<'a> {
    let folder = match source {
        PageSource::File { folder, .. } => Some(folder.clone()),
        PageSource::Html(_) => None,
    };
    let builder = WebViewBuilder::new()
        .with_navigation_handler(move |url| stays_local(&url, folder.as_deref()))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny);
    match source {
        PageSource::File { .. } => builder,
        PageSource::Html(html) => builder.with_html(html.clone()),
    }
}

fn load_local(view: &WebView, source: &PageSource) {
    if let PageSource::File { file, folder } = source {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy()));
        let readable = NSURL::fileURLWithPath_isDirectory(
            &NSString::from_str(&folder.to_string_lossy()),
            true,
        );
        // SAFETY: both are file URLs made just now, and this is the main thread the web view was
        // made on. The folder is the one read access is given to; the navigation handler allows
        // the file's own URL, which is inside it.
        unsafe {
            view.webview()
                .loadFileURL_allowingReadAccessToURL(&url, &readable);
        }
    }
}

fn retire(view: &WebView) {
    let web = view.webview();
    // SAFETY: plain WKWebView calls on the main thread, where web views are made and dropped.
    unsafe {
        web.stopLoading();
        web.loadHTMLString_baseURL(&NSString::from_str(""), None);
    }
}

pub enum PanelPage<'a> {
    Local(&'a PageSource),
    Live(&'a str),
}

pub struct PanelHooks {
    pub title: Box<dyn Fn(String)>,
    pub leave: Box<dyn Fn(String)>,
}

pub struct PanelWeb {
    view: WebView,
    gpui: Weak<NSView>,
    placed: Cell<Option<(Frame, bool)>>,
}

impl PanelWeb {
    pub fn open(
        window: &impl HasWindowHandle,
        page: PanelPage<'_>,
        hooks: PanelHooks,
    ) -> Result<PanelWeb, String> {
        MainThreadMarker::new().ok_or("the page panel opens on the main thread")?;
        let handle = window
            .window_handle()
            .map_err(|e| format!("the Chat window has no handle: {e}"))?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return Err("the Chat window isn't an AppKit window".to_owned());
        };
        // SAFETY: GPUI's handle names its live view, on the main thread; it is only borrowed here to
        // reach its superview, which comes back retained.
        let gpui_view = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
        let gpui = Weak::from_retained(&gpui_view.retain());
        // SAFETY: an AppKit view call on the main thread, checked above.
        let content =
            unsafe { gpui_view.superview() }.ok_or("the Chat window has no content view")?;
        let PanelHooks { title, leave } = hooks;
        let builder = match page {
            PanelPage::Local(source) => local_builder(source),
            PanelPage::Live(url) => WebViewBuilder::new()
                .with_navigation_handler(|url| live_allows(&url))
                .with_new_window_req_handler(move |url, _| {
                    if opens_in_browser(&url) {
                        leave(url);
                    }
                    NewWindowResponse::Deny
                })
                .with_data_store_identifier(LIVE_STORE)
                .with_url(url),
        };
        let view = builder
            .with_document_title_changed_handler(move |t| {
                if !t.is_empty() {
                    title(t);
                }
            })
            .with_bounds(Rect {
                position: LogicalPosition::new(0.0, 0.0).into(),
                size: LogicalSize::new(1.0, 1.0).into(),
            })
            .with_visible(false)
            .with_focused(false)
            .build_as_child(&ViewHandle(content))
            .map_err(|e| format!("the web view couldn't be made: {e}"))?;
        if let PanelPage::Local(source) = page {
            load_local(&view, source);
        }
        Ok(PanelWeb {
            view,
            gpui,
            placed: Cell::new(None),
        })
    }

    pub fn place(&self, bounds: Frame, visible: bool, scale: f64) {
        let web = self.view.webview();
        // SAFETY: AppKit view calls on the main thread, which is where GPUI paints.
        let Some(parent) = (unsafe { web.superview() }) else {
            return;
        };
        let frame = appkit_frame(
            bounds,
            parent.bounds().size.height,
            parent.isFlipped(),
            scale,
        );
        if self.placed.get() == Some((frame, visible)) {
            return;
        }
        self.placed.set(Some((frame, visible)));
        let rect = NSRect::new(
            NSPoint::new(frame.x, frame.y),
            NSSize::new(frame.width, frame.height),
        );
        web.setFrame(rect);
        web.setHidden(!visible);
    }

    pub fn take_keys_back(&self) {
        let Some(gpui) = self.gpui.load() else {
            return;
        };
        let Some(window) = gpui.window() else {
            return;
        };
        let has_keys = window.firstResponder().is_some_and(|r| {
            std::ptr::eq(
                Retained::as_ptr(&r).cast::<NSView>(),
                Retained::as_ptr(&gpui),
            )
        });
        if !has_keys {
            window.makeFirstResponder(Some(&gpui));
        }
    }

    pub fn reload(&self) {
        let _ = self.view.reload();
    }

    pub fn hide(&self) {
        if let Some((frame, true)) = self.placed.get() {
            self.placed.set(Some((frame, false)));
            self.view.webview().setHidden(true);
        }
    }
}

impl Drop for PanelWeb {
    fn drop(&mut self) {
        retire(&self.view);
        self.view.webview().setHidden(true);
    }
}
