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
