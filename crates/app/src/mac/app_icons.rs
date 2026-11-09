//! The tools' app icons, the way the Swift `ToolIcon` takes them: the installed app's icon from
//! the system (`NSWorkspace`), found by bundle id, drawn at twice the row's 14 pt into a bitmap and
//! handed to the kit as PNG bytes in memory. Read once per tool; a tool whose app isn't installed
//! keeps the kit's own mark. Nothing is written to disk, and no logo is in the repository.

use std::ptr;
use std::sync::{Arc, Once};

use gpui::{Image, ImageFormat};
use notchling_kit::metric::TOOL_ICON;
use notchling_kit::ui::{first_installed, set_tool_image, ToolMark};
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace,
    NSGraphicsContext, NSWorkspace,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

/// The icon's pixels a side: twice its point size.
const PIXELS: usize = (TOOL_ICON as usize) * 2;

/// The app's icon as PNG bytes `pixels` square, or `None` when no app has the bundle id.
fn icon_png(bundle_id: &str, pixels: usize) -> Option<Vec<u8>> {
    let workspace = NSWorkspace::sharedWorkspace();
    let url = workspace.URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
    let path = url.path()?;
    let icon = workspace.iconForFile(&path);
    let side = pixels as isize;
    // SAFETY: no planes are passed (the rep allocates its own), and the color space is a system
    // constant.
    let rep: Retained<NSBitmapImageRep> = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            ptr::null_mut(),
            side,
            side,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    icon.drawInRect_fromRect_operation_fraction(
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(side as f64, side as f64),
        ),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
        NSCompositingOperation::SourceOver,
        1.0,
    );
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    // SAFETY: an empty properties dictionary is what the PNG type takes.
    let data = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    Some(data.to_vec())
}

/// Hands the kit each installed tool's app icon, once per run (the main thread, before a view
/// that draws them).
pub fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        for tool in [ToolMark::Claude, ToolMark::Codex, ToolMark::Antigravity] {
            let png = first_installed(tool.bundle_ids(), |id| icon_png(id, PIXELS));
            if let Some(bytes) = png {
                set_tool_image(tool, Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
            }
        }
    });
}
