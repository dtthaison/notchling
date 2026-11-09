//! The tools' app icons, handed over by the app: the kit draws them in [`tool_icon`] where it has
//! one and its own mark where it has none (the app isn't installed, or this isn't macOS). The kit
//! never reads a bundle itself, and no logo is stored anywhere in the repository.
//!
//! [`tool_icon`]: super::tool_icon

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use gpui::Image;

use super::widgets::ToolMark;

fn images() -> &'static Mutex<HashMap<ToolMark, Arc<Image>>> {
    static IMAGES: OnceLock<Mutex<HashMap<ToolMark, Arc<Image>>>> = OnceLock::new();
    IMAGES.get_or_init(Mutex::default)
}

/// Keeps `image` as the tool's icon, replacing an earlier one.
pub fn set_tool_image(tool: ToolMark, image: Arc<Image>) {
    if let Ok(mut all) = images().lock() {
        all.insert(tool, image);
    }
}

/// The tool's icon, if the app handed one over.
pub(super) fn tool_image(tool: ToolMark) -> Option<Arc<Image>> {
    images().lock().ok()?.get(&tool).cloned()
}

/// The first of `ids` that `lookup` finds an app for, with what it found: the Swift
/// `ToolIcon.image(for:)` loop, pure so it is tested without any app installed.
pub fn first_installed<T>(ids: &[&str], mut lookup: impl FnMut(&str) -> Option<T>) -> Option<T> {
    ids.iter().find_map(|id| lookup(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tool_names_the_apps_swift_does() {
        assert_eq!(
            ToolMark::Claude.bundle_ids(),
            ["com.anthropic.claudefordesktop"]
        );
        assert_eq!(ToolMark::Codex.bundle_ids(), ["com.openai.codex"]);
        assert_eq!(
            ToolMark::Antigravity.bundle_ids(),
            ["com.google.antigravity", "com.google.antigravity-ide"]
        );
    }

    #[test]
    fn the_first_installed_app_wins_and_none_is_none() {
        let ids = ToolMark::Antigravity.bundle_ids();
        let only_ide = |id: &str| (id == "com.google.antigravity-ide").then_some("/Apps/IDE.app");
        assert_eq!(first_installed(ids, only_ide), Some("/Apps/IDE.app"));
        let both = |id: &str| Some(id.to_owned());
        assert_eq!(
            first_installed(ids, both).as_deref(),
            Some("com.google.antigravity")
        );
        assert_eq!(first_installed(ids, |_| None::<u8>), None);
        assert_eq!(first_installed(&[], both), None);
    }

    #[test]
    fn a_handed_over_image_is_what_tool_image_returns() {
        let image = Arc::new(Image::from_bytes(gpui::ImageFormat::Png, vec![1, 2, 3]));
        set_tool_image(ToolMark::Antigravity, image.clone());
        assert_eq!(tool_image(ToolMark::Antigravity), Some(image));
    }
}
