//! The design kit: tokens and parts shared by every view (part U0).
//!
//! Ported from `legacy/macos/Sources/Notchling/DesignKit/ChatKit.swift`; the spec is
//! `docs/chat-kit.md`. Two layers:
//!
//! - **Tokens**, pure data with no dependencies, tested on every system: [`Ink`] (colors by meaning,
//!   light and dark), [`TypeRole`] (the type scale), [`metric`], [`Motion`], [`Icon`] (SF Symbol names
//!   and the Lucide SVGs drawn in their place, as data), [`part`] (what each part looks like in each state), [`labels`] (the ? button's part
//!   names: the state, where a badge sits) and [`text_edit`] (the reply box's editing model:
//!   selection, marked text, undo). Only `ink` reads the palette.
//! - **Widgets** over GPUI in [`ui`], macOS only for now (`LINT-GPUI-MACOS-ONLY`): buttons in the
//!   kit's styles, session rows, options, pills, avatars, a field, a label, and [`ui::KitGallery`],
//!   every part in every state for a render.
//!
//! Views use the tokens, never a raw color or size.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod appearance;
pub mod color;
pub mod icon;
pub mod ink;
pub mod labels;
pub mod metric;
pub mod motion;
mod palette;
pub mod part;
pub mod render_map;
pub mod resize;
pub mod scrollbar;
pub mod text_edit;
pub mod typescale;
#[cfg(target_os = "macos")]
pub mod ui;

pub use appearance::Appearance;
pub use color::Color;
pub use icon::{Icon, IconSize};
pub use ink::{Ink, Status};
pub use motion::{Curve, Motion};
pub use part::{ButtonKind, PartState};
pub use typescale::{sentence, TypeRole, TypeSpec, Weight};

/// The crate's short name.
pub fn crate_name() -> &'static str {
    "kit"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_itself() {
        assert_eq!(crate_name(), "kit");
    }
}
