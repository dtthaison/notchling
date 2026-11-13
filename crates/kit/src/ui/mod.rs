mod gallery;
mod icons;
mod menu;
mod parts;
mod resize;
mod scrollbar;
mod scrollbar_track;
mod spin_layer;
mod spin_native;
mod text_editor;
mod tool_images;
mod widgets;

pub use gallery::KitGallery;
pub use icons::{icon, icon_in, spinner, spinner_in, KitAssets};
pub use menu::{
    menu_note, menu_panel, menu_panel_wide, menu_row, menu_rule, menu_section, MenuItem,
};
pub use parts::{parts_button, parts_on, parts_tip, set_parts, toggle_parts, KitPart, RegionScope};
pub use resize::{resize_edges, TopBorder};
pub use scrollbar::{scrollable, scrollable_list, PinScrollbars, Scrollable, ThumbAt, ThumbRecord};
pub use spin_layer::{drop_spinner_host, host_spinners, spin_host, SpinLayer, SpinSlot, SpinSlots};
pub use spin_native::{
    diff as spin_diff, spin_period, spin_phase, Diff as SpinDiff, SpinGate, SpinSink,
};
pub use text_editor::{TextEditor, TextEditorEvent};
pub use tool_images::{first_installed, set_tool_image};
pub use widgets::{
    account_line, avatar, button, button_with, card, claude_mark, field, icon_button, label,
    option_row, phone_mark, session_row, session_row_marked, status_pill, tool_icon, ToolMark,
};

use gpui::{px, FontWeight, Hsla, Rgba, Styled, Window, WindowAppearance};

use crate::part::Look;
use crate::{Appearance, Color, Ink, TypeRole, Weight};

pub const SYSTEM_FONT: &str = ".SystemUIFont";
pub const MONO_FONT: &str = "Menlo";

pub fn rgba(color: Color) -> Rgba {
    let [r, g, b, a] = color.channels();
    Rgba { r, g, b, a }
}

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

pub fn appearance_of(appearance: WindowAppearance) -> Appearance {
    match appearance {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
    }
}

pub fn window_appearance(window: &Window) -> Appearance {
    crate::preference().resolve(appearance_of(window.appearance()))
}

pub trait KitStyled: Styled + Sized {
    fn kit_type(self, role: TypeRole) -> Self {
        let spec = role.spec();
        let family = if spec.mono { MONO_FONT } else { SYSTEM_FONT };
        self.font_family(family)
            .text_size(px(spec.size))
            .font_weight(font_weight(spec.weight))
    }

    fn ink(self, ink: Ink, appearance: Appearance) -> Self {
        self.text_color(hsla(ink.color(appearance)))
    }

    fn fill(self, ink: Ink, appearance: Appearance) -> Self {
        self.bg(hsla(ink.color(appearance)))
    }

    fn elide(self) -> Self {
        self.overflow_hidden().text_ellipsis().line_clamp(1)
    }

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
