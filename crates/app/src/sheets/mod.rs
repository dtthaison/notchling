//! Chat's sheets and pop-ups (parts U7 and U8): banners, the New Session sheet with its Trust
//! Folder pop-up, and Settings. The view models are pure and build on every system; the GPUI
//! views are macOS only (`LINT-GPUI-MACOS-ONLY`).

pub mod banner;
pub mod new_session;
pub mod renders;
pub mod settings;
#[cfg(target_os = "macos")]
pub mod settings_view;
#[cfg(target_os = "macos")]
pub mod view;
