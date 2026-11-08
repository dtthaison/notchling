//! A notch render's setting (part U10): the model and scene from `renders`, drawn over a stand-in
//! desktop with its menu bar, and for Tab Perch a stand-in for the frontmost window.

use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyView, App, Div};
use notchling_kit::metric::{RADIUS_SM, SIDE_PADDING};
use notchling_kit::ui::{hsla, KitStyled};
use notchling_kit::{Ink, TypeRole};

use super::cards::{text, A};
use super::perch::AxFrame;
use super::renders;
use super::view::NotchView;
use crate::actions::Actions;

/// `--render notch-<state>`: the model over the fixture, drawn on a backdrop, with what the
/// scene sets: acting, the pet, picks already made.
pub fn build_render(name: &str, cx: &mut App) -> Option<AnyView> {
    let shown = renders::shown(name)?;
    let scene = renders::scene(shown);
    let actions = Arc::new(Actions::fixed(scene.acting));
    Some(
        cx.new(|cx| {
            let mut view = NotchView::new(scene.model, true, actions, cx);
            view.forced_pet = scene.pet;
            view.walk.motion = scene.motion;
            view.perch = scene.perch;
            view.scene_window = scene.window;
            view.sync_draft();
            if let Some(q) = scene.question {
                for (tab, option) in scene.picks {
                    view.draft.select_tab(tab);
                    view.draft.choose(&q, option);
                }
            }
            view.draft.select_tab(scene.tab);
            view
        })
        .into(),
    )
}

impl NotchView {
    /// A render's stand-in for the desktop: a menu bar the housing's height, the desktop below.
    pub(super) fn backdrop(&self) -> Div {
        div()
            .absolute()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(self.model.notch.h as f32))
                    .w_full()
                    .fill(Ink::Raised, A)
                    .px(px(SIDE_PADDING))
                    .flex()
                    .items_center()
                    .child(text("Notchling", TypeRole::Label, Ink::Text)),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .fill(Ink::Sidebar, A)
                    .children(self.scene_window.map(|w| {
                        // The frame is from the top of the screen; this part starts below the
                        // menu bar.
                        stand_in_window(AxFrame {
                            y: w.y - self.model.notch.h,
                            ..w
                        })
                    })),
            )
    }
}

/// A stand-in for the frontmost window a render's pet perches on: a card with traffic lights, at
/// its frame in the canvas (y down).
pub fn stand_in_window(frame: AxFrame) -> Div {
    let light = |ink: Ink| div().size(px(12.0)).rounded_full().fill(ink, A);
    div()
        .absolute()
        .left(px(frame.x as f32))
        .top(px(frame.y as f32))
        .w(px(frame.w as f32))
        .h(px(frame.h as f32))
        .rounded(px(RADIUS_SM * 2.0))
        .fill(Ink::Card, A)
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(A)))
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(light(Ink::Needs))
                .child(light(Ink::Accent))
                .child(light(Ink::Done))
                .child(div().w(px(70.0)))
                .child(text("Terminal", TypeRole::Label, Ink::TextMuted)),
        )
        .child(
            div()
                .px(px(12.0))
                .child(text("~/work/notchling $", TypeRole::Mono, Ink::TextMuted)),
        )
}
