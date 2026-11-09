//! The widget layer over GPUI: the tokens applied. macOS only for now (`LINT-GPUI-MACOS-ONLY`).
//!
//! Views style through [`KitStyled`] (`kit_type`, `ink`, `fill`, `look`) and build parts with the
//! functions in `widgets`, never with a raw color or size. [`TextEditor`] is the text box with a
//! caret, a selection and the input method (the reply box). [`KitPart`] names an element for the
//! ? button (`part`, `part_at`, `part_inside`), whose state is the [`crate::labels::Labels`]
//! global (`parts_on`, `toggle_parts`). [`icon`] draws an icon's Lucide SVG (served by
//! [`KitAssets`]) where the Swift app draws an SF Symbol. What GPUI can't do yet is noted where it
//! matters: no letter spacing (section headers are uppercase but not tracked), no press scale.

mod gallery;
mod icons;
mod parts;
mod resize;
mod text_editor;
mod widgets;

pub use gallery::KitGallery;
pub use icons::{icon, KitAssets};
pub use parts::{parts_button, parts_on, parts_tip, set_parts, toggle_parts, KitPart};
pub use resize::{resize_edges, TopBorder};
pub use text_editor::{TextEditor, TextEditorEvent};
pub use widgets::{
    account_line, avatar, button, button_with, card, claude_mark, field, label, option_row,
    phone_mark, session_row, session_row_marked, status_pill, tool_icon, ToolMark,
};

use gpui::{px, FontWeight, Hsla, Rgba, Styled, Window, WindowAppearance};

use crate::part::Look;
use crate::{Appearance, Color, Ink, TypeRole, Weight};

/// GPUI's name for the system UI font (San Francisco on macOS).
pub const SYSTEM_FONT: &str = ".SystemUIFont";
/// The monospaced family. SF Mono isn't installed system-wide, so Menlo, which every Mac has.
pub const MONO_FONT: &str = "Menlo";

/// A kit color as GPUI's RGBA.
pub fn rgba(color: Color) -> Rgba {
    let [r, g, b, a] = color.channels();
    Rgba { r, g, b, a }
}

/// A kit color as GPUI's HSLA, which its style setters take.
pub fn hsla(color: Color) -> Hsla {
    rgba(color).into()
}

pub fn font_weight(weight: Weight) -> FontWeight {
    match weight {
        Weight::Regular => FontWeight::NORMAL,
        Weight::Medium => FontWeight::MEDIUM,
        Weight::Semibold => FontWeight::SEMIBOLD,
        Weight::Bold => FontWeight::BOLD,
    }
}

/// Light or dark from what GPUI reports; the vibrant forms count as their base.
pub fn appearance_of(appearance: WindowAppearance) -> Appearance {
    match appearance {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
    }
}

/// The window's current appearance, which follows the system.
pub fn window_appearance(window: &Window) -> Appearance {
    appearance_of(window.appearance())
}

/// The kit's style setters on any GPUI element (and on a `StyleRefinement` inside `hover`/`active`).
pub trait KitStyled: Styled + Sized {
    /// Font, size and weight of a type role.
    fn kit_type(self, role: TypeRole) -> Self {
        let spec = role.spec();
        let family = if spec.mono { MONO_FONT } else { SYSTEM_FONT };
        self.font_family(family)
            .text_size(px(spec.size))
            .font_weight(font_weight(spec.weight))
    }

    /// Text in an ink.
    fn ink(self, ink: Ink, appearance: Appearance) -> Self {
        self.text_color(hsla(ink.color(appearance)))
    }

    /// Background in an ink.
    fn fill(self, ink: Ink, appearance: Appearance) -> Self {
        self.bg(hsla(ink.color(appearance)))
    }

    /// One line, cut with an ellipsis where it runs out of room. GPUI 0.2.2's `truncate()` sets
    /// `nowrap` too, and a nowrap text keeps its first (max-content) measurement, so the ellipsis
    /// never draws and the text is clipped mid-word at the edge; without `nowrap` the definite
    /// width is measured again and the text is cut to it. The element needs a definite width (a
    /// `flex_1` item with `min_w(0)`, or `w_full`), as with `truncate()`.
    fn elide(self) -> Self {
        self.overflow_hidden().text_ellipsis()
    }

    /// A part's look in one state: fill, edge (the focus ring wins over the border), text, opacity.
    fn look(mut self, look: &Look) -> Self {
        if let Some(fill) = look.fill {
            self = self.bg(hsla(fill));
        }
        if let Some(edge) = look.edge() {
            self = self.border(px(edge.width)).border_color(hsla(edge.color));
        }
        self.text_color(hsla(look.text)).opacity(look.opacity)
    }
}

impl<T: Styled> KitStyled for T {}
