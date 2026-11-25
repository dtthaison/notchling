use std::path::Path;

use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSView, NSWindow};
#[allow(deprecated)]
use objc2_core_graphics::{
    CGRectNull, CGWindowImageOption, CGWindowListCreateImage, CGWindowListOption,
};
use objc2_foundation::NSDictionary;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capture {
    pub width: usize,
    pub height: usize,
    pub bytes: usize,
}

pub fn ns_window(window: &gpui::Window) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: GPUI hands out the pointer to its live NSView for this window.
    let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

#[allow(deprecated)]
pub fn write_png(window: &NSWindow, path: &Path) -> Result<Capture, String> {
    let id = u32::try_from(window.windowNumber()).map_err(|_| "no window number".to_owned())?;
    let image = CGWindowListCreateImage(
        // SAFETY: a plain static rectangle constant.
        unsafe { CGRectNull },
        CGWindowListOption::OptionIncludingWindow,
        id,
        CGWindowImageOption::BoundsIgnoreFraming | CGWindowImageOption::BestResolution,
    )
    .ok_or_else(|| "CGWindowListCreateImage returned nothing".to_owned())?;
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
    let width = usize::try_from(rep.pixelsWide()).unwrap_or(0);
    let height = usize::try_from(rep.pixelsHigh()).unwrap_or(0);
    // SAFETY: an empty properties dictionary is what the PNG encoder expects for defaults.
    let data = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or_else(|| "PNG encoding failed".to_owned())?;
    let bytes = data.to_vec();
    if let Some(folder) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    std::fs::write(path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Capture {
        width,
        height,
        bytes: bytes.len(),
    })
}
