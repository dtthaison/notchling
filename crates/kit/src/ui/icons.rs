//! Icons drawn: the kit's Lucide SVGs (`crate::icon`) through GPUI's asset source, in an ink.

use std::borrow::Cow;

use gpui::{div, prelude::*, px, svg, AssetSource, Div, FontWeight, SharedString};

use super::KitStyled;
use crate::icon::{asset, assets, svg_box};
use crate::{Appearance, Icon, Ink};

/// Serves the embedded icons. An app hands it to `Application::with_assets`, or `svg()` draws
/// nothing.
pub struct KitAssets;

impl AssetSource for KitAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(asset(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(assets(path).map(SharedString::from).collect())
    }
}

/// An icon the size of an SF Symbol of `points`, in `ink`: its Lucide SVG, else its text glyph.
pub fn icon(icon: Icon, points: f32, ink: Ink, a: Appearance) -> Div {
    let holder = div().flex_shrink_0().flex().items_center().justify_center();
    match icon.svg() {
        Some(path) => {
            let size = px(svg_box(points));
            holder
                .size(size)
                .child(svg().path(path).size(size).ink(ink, a))
        }
        None => holder
            .text_size(px(points))
            .font_weight(FontWeight::MEDIUM)
            .ink(ink, a)
            .child(icon.glyph()),
    }
}
