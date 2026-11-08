//! Search (part U3): the ⌘⇧P panel over saved Artifact pages and conversations, the Swift app's
//! `SearchPanel.swift`. The pure parts build and are tested on every system: [`wire`] (the
//! daemon's search ops and their tolerant readers), [`library`] (what the panel asks the daemon),
//! [`model`] (what the panel shows and what each key does), [`hotkey`] (the global shortcut
//! behind a trait) and [`renders`] (the `--render` names). The GPUI panel and the Carbon hot key
//! are macOS only (`LINT-GPUI-MACOS-ONLY`): [`view`] and [`panel`] here, `mac::hotkey`.

pub mod hotkey;
pub mod library;
pub mod model;
#[cfg(target_os = "macos")]
pub mod panel;
pub mod renders;
#[cfg(target_os = "macos")]
pub mod view;
pub mod wire;
