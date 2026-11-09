//! Icons drawn: the kit's Lucide SVGs (`crate::icon`) through GPUI's asset source, in an ink.

use std::borrow::Cow;
use std::f32::consts::TAU;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, radians, svg, Animation, AnimationExt, AssetSource, Div, ElementId,
    FontWeight, SharedString, Transformation,
};

use super::hsla;
use crate::icon::{asset, assets, svg_box};
use crate::{Appearance, Color, Icon, Ink};

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
    icon_in(icon, points, ink.color(a))
}

/// [`icon`] in a color a part's look already chose (a badge's text, a button's). An SVG takes its
/// color from its own style, not from the text around it, so an icon inside a button needs this.
pub fn icon_in(icon: Icon, points: f32, color: Color) -> Div {
    let holder = div().flex_shrink_0().flex().items_center().justify_center();
    match icon.svg() {
        Some(path) => {
            let size = px(svg_box(points));
            holder
                .size(size)
                .child(svg().path(path).size(size).text_color(hsla(color)))
        }
        None => holder
            .text_size(px(points))
            .font_weight(FontWeight::MEDIUM)
            .text_color(hsla(color))
            .child(icon.glyph()),
    }
}

/// How long a spinner takes to turn once.
const SPIN: Duration = Duration::from_millis(1000);
/// The turn is stepped in this many parts, so it ticks like the macOS spinner.
const SPIN_STEPS: f32 = 8.0;
/// Lucide's `loader` fills 20 of its 24 units, so its box is this much bigger than the spinner.
const SPIN_BOX: f32 = 1.2;

/// How far round a spinner is `t` (0 to 1) of the way through its turn, in turns: the nearest
/// step at or below, so it ticks in 45 degree steps.
fn spin_turns(t: f32) -> f32 {
    (t.clamp(0.0, 1.0) * SPIN_STEPS)
        .floor()
        .min(SPIN_STEPS - 1.0)
        / SPIN_STEPS
}

/// The loading spinner (the Swift `ProgressView().controlSize(...)`: 10 pt mini, 16 pt small):
/// Lucide's eight-spoke `loader`, turning once a second in eight steps of 45 degrees. `id` names
/// its animation, so two spinners in one place need two ids. GPUI draws frames only for what is
/// on screen, so nothing animates while no spinner is drawn.
pub fn spinner(id: impl Into<ElementId>, diameter: f32, ink: Ink, a: Appearance) -> Div {
    let size = px(diameter * SPIN_BOX);
    let turning = svg()
        .path(Icon::Spinner.svg().unwrap_or("icons/loader.svg"))
        .size(size)
        .text_color(hsla(ink.color(a)))
        .with_animation(id, Animation::new(SPIN).repeat(), |svg, t| {
            svg.with_transformation(Transformation::rotate(radians(spin_turns(t) * TAU)))
        });
    div()
        .flex_shrink_0()
        .size(size)
        .flex()
        .items_center()
        .justify_center()
        .child(turning)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spinner_turns_in_eight_steps_of_45_degrees() {
        assert_eq!(spin_turns(0.0), 0.0);
        assert_eq!(spin_turns(0.124), 0.0);
        assert_eq!(spin_turns(0.125), 0.125);
        assert_eq!(spin_turns(0.5), 0.5);
        assert_eq!(spin_turns(0.999), 0.875);
        assert_eq!(
            spin_turns(1.0),
            0.875,
            "the turn ends where the next begins"
        );
    }
}
