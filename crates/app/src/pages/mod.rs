//! Pages (part U4): the Artifacts window over the daemon's pages library, the Swift app's
//! `PagesWindow.swift`, and each page opened from its local copy in a `wry` web view in a window
//! of its own, since GPUI has no web view (decision 0005). The pure parts build and are tested on
//! every system: [`model`] (the list, its search and selection), [`dates`] (the dates the rows
//! show) and [`renders`] (the `--render` name). The GPUI window ([`view`], [`window`]) and the
//! web view ([`web`]) are macOS only (`LINT-GPUI-MACOS-ONLY`).

pub mod dates;
pub mod local;
pub mod model;
pub mod renders;
#[cfg(target_os = "macos")]
pub mod view;
#[cfg(target_os = "macos")]
pub mod web;
#[cfg(target_os = "macos")]
pub mod window;
