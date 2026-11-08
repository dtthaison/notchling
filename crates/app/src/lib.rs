//! The desktop app: windows, views and wiring (part U0), `--render` for agents (part U10), and the
//! read-only Chat window (part U1, Milestone 1).
//!
//! The pure parts build and are tested on every system: [`cli`] (the arguments), [`menu`] (the
//! menu bar item's entries), [`dock`] (when the app shows in the Dock, and the main menu it gets),
//! [`views`] (the views `--render` knows) and [`chat`]'s view models (sessions, rows, header,
//! inspector) over the engine. The GPUI and AppKit side is [`mac`] and `chat::view`, macOS only
//! for now (`LINT-GPUI-MACOS-ONLY`): Linux needs system libraries CI doesn't install yet, and
//! Windows is unverified. There `main` prints [`NOT_SUPPORTED`] and exits 0.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod accounts;
pub mod acting;
pub mod actions;
pub mod chat;
pub mod cli;
pub mod dock;
#[cfg(target_os = "macos")]
pub mod mac;
pub mod menu;
pub mod notch;
pub mod pages;
pub mod phone;
pub mod search;
pub mod session_source;
pub mod sheets;
pub mod switches;
#[cfg(test)]
mod testutil;
pub mod tools;
pub mod transcripts;
pub mod vault;
pub mod views;

/// What `main` prints on a system the app isn't built for yet.
pub const NOT_SUPPORTED: &str = "notchling-app: not supported yet on this system";

/// The crate's short name.
pub fn crate_name() -> &'static str {
    "app"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_itself() {
        assert_eq!(crate_name(), "app");
    }
}
