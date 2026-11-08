//! Building a named view (`crate::views`) as a GPUI entity, for `--render`.

use gpui::{prelude::*, AnyView, App, Window};
use notchling_kit::ui::KitGallery;
use notchling_kit::Appearance;

use super::window::ShellView;

/// The view for a name from `crate::views::NAMES`, or `None` for a name it doesn't know.
pub fn build(name: &str, window: &mut Window, cx: &mut App) -> Option<AnyView> {
    let (kind, appearance) = match name.rsplit_once('-') {
        Some((kind, suffix)) if Appearance::parse(suffix).is_some() => {
            (kind, Appearance::parse(suffix))
        }
        _ => (name, None),
    };
    match kind {
        "kit-gallery" => Some(cx.new(|cx| KitGallery::new(appearance, window, cx)).into()),
        "shell" => Some(cx.new(|cx| ShellView::new(appearance, window, cx)).into()),
        _ => None,
    }
}
