//! Pages (part U4): the Artifacts window over the daemon's pages library, the Swift app's
//! `PagesWindow.swift`, and each page opened from its local copy in a `wry` web view in a window
//! of its own, since GPUI has no web view (decision 0005). The pure parts build and are tested on
//! every system: [`model`] (the list, its search and selection), [`dates`] (the dates the rows
//! show), [`local`] (what a copy loads as, and where its page may go), [`set`] (the page windows
//! held) and [`renders`] (the `--render` name). The GPUI window ([`view`], [`window`]) and the
//! web view ([`web`]) are macOS only (`LINT-GPUI-MACOS-ONLY`).

pub mod dates;
pub mod local;
pub mod model;
pub mod renders;
pub mod set;
#[cfg(target_os = "macos")]
pub mod view;
#[cfg(target_os = "macos")]
pub mod web;
#[cfg(target_os = "macos")]
pub mod window;
