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

pub fn build_render(name: &str, cx: &mut App) -> Option<AnyView> {
    let shown = renders::shown(name)?;
    if std::env::var_os(crate::chat::view::PARTS_ENV).is_some_and(|v| v == "1") {
        notchling_kit::ui::set_parts(cx, true);
    }
    Some(build_scene(renders::scene(shown), cx))
}

pub fn build_scene(scene: renders::Scene, cx: &mut App) -> AnyView {
    let actions = Arc::new(Actions::fixed(scene.acting));
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
        if std::env::var_os(crate::mac::render::BENCH_ENV).is_some() {
            cx.spawn(async move |this, cx| loop {
                let stepped = this.update(cx, |view, cx| {
                    if view.step_pet(super::source::now_ms() as f64 / 1000.0) {
                        cx.notify();
                    }
                });
                if stepped.is_err() {
                    break;
                }
                cx.background_executor().timer(super::panel::TICK).await;
            })
            .detach();
        }
        view
    })
    .into()
}

impl NotchView {
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
                        stand_in_window(AxFrame {
                            y: w.y - self.model.notch.h,
                            ..w
                        })
                    })),
            )
    }
}

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
