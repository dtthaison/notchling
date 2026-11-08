//! A page in a `wry` web view in its own window (part U4, macOS; the Phase 1 spike's S2, which
//! showed wry builds into a plain `NSView` and doesn't fight GPUI's run loop). The window is a
//! titled, closable, resizable `NSWindow` of ours; the web view is built into its content view
//! through the raw window handle, with no `tao`.
//!
//! The page is the local copy and nothing else: a link it holds to somewhere else is refused,
//! since leaving for the browser means opening another app (a capability change, `CAP-PROCESS`,
//! for the maintainer), and a `target=_blank` window is denied.

use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBackingStoreType, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
};
use wry::{NewWindowResponse, WebView, WebViewBuilder};

use super::local::PageSource;

/// The window's size when it opens, and the least it can be.
const SIZE: (f64, f64) = (960.0, 720.0);
const MIN_SIZE: (f64, f64) = (480.0, 360.0);
/// Each page window opens a step down and right of the one before.
const STEP: f64 = 28.0;

/// A page window's content view, as the raw handle wry builds into.
struct ViewHandle(Retained<NSView>);

impl HasWindowHandle for ViewHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let ptr = NonNull::from(&*self.0).cast();
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(ptr));
        // SAFETY: the view is retained by this handle for as long as the handle is borrowed.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

/// Whether a navigation stays on the local copy.
pub fn stays_local(url: &str) -> bool {
    url.starts_with("file:") || url.starts_with("about:")
}

/// An open page window; dropping it closes the window and lets go of the web view.
pub struct PageWindow {
    window: Retained<NSWindow>,
    _view: WebView,
}

impl PageWindow {
    /// Opens `source` in a new window titled `title`, the `nth` one down and right of the first.
    pub fn open(title: &str, source: &PageSource, nth: usize) -> Result<PageWindow, String> {
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
        let titled = window.clone();
        let builder = WebViewBuilder::new()
            .with_navigation_handler(|url| stays_local(&url))
            .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
            .with_document_title_changed_handler(move |t| {
                if !t.is_empty() {
                    titled.setTitle(&NSString::from_str(&t));
                }
            });
        let builder = match source {
            PageSource::Url(url) => builder.with_url(url.clone()),
            PageSource::Html(html) => builder.with_html(html.clone()),
        };
        let view = builder
            .build(&ViewHandle(content))
            .map_err(|e| format!("the web view couldn't be made: {e}"))?;
        window.makeKeyAndOrderFront(None);
        Ok(PageWindow {
            window,
            _view: view,
        })
    }

    /// Whether the person still has it open.
    pub fn is_open(&self) -> bool {
        self.window.isVisible()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_local_copy_is_navigated_to() {
        assert!(stays_local("file:///Users/me/pages/a/index.html"));
        assert!(stays_local("about:blank"));
        assert!(!stays_local("https://claude.ai/artifact/x"));
        assert!(!stays_local("http://example.com"));
        assert!(!stays_local("javascript:alert(1)"));
    }
}
