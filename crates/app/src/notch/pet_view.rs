use gpui::{div, prelude::*, px, Div, FontWeight};
use notchling_kit::ui::KitStyled;
use notchling_kit::{Icon, Ink};

use super::answering::badge;
use super::cards::A;
use super::layout::{NotchState, Size, CANVAS_WIDTH};
use super::perch::{self, PetMode};
use super::pet::{self, Point, RADIUS};
use super::view::NotchView;
use notchling_engine::sessions::AgentGroup;

const GLYPH: f32 = 2.0 * RADIUS as f32;

fn glyph() -> Div {
    div()
        .size(px(GLYPH))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(15.0))
        .font_weight(FontWeight::BOLD)
        .ink(Ink::Accent, A)
        .child(Icon::Claude.glyph())
}

fn at(element: Div, p: Point, w: f32, h: f32) -> Div {
    div()
        .absolute()
        .left(px(p.x as f32 - w / 2.0))
        .top(px(p.y as f32 - h / 2.0))
        .w(px(w))
        .h(px(h))
        .flex()
        .items_center()
        .justify_center()
        .child(element)
}

impl NotchView {
    pub(super) fn pet_mode(&self) -> PetMode {
        self.forced_pet.unwrap_or_else(|| {
            perch::mode(
                self.prefs.show_pet,
                self.model.count(AgentGroup::Working),
                self.model.count(AgentGroup::NeedsInput),
            )
        })
    }

    pub(super) fn step_pet(&mut self, now_s: f64) -> bool {
        let collapsed = self.model.state == NotchState::Collapsed;
        let rolling = collapsed && self.pet_mode() == PetMode::Walking;
        let shape = self.model.size(&NotchState::Collapsed);
        let needs = self.model.count(AgentGroup::NeedsInput) > 0;
        let before = self.walk.motion;
        self.walk.step(now_s, rolling, needs, shape);
        before != self.walk.motion
    }

    pub(super) fn pet_layer(&self, size: Size) -> Option<Div> {
        if self.model.state != NotchState::Collapsed {
            return None;
        }
        let needs = self.model.count(AgentGroup::NeedsInput);
        match self.pet_mode() {
            PetMode::Hidden => None,
            PetMode::Walking => {
                let m = self.walk.motion;
                let spot = pet::Track::new(size).point(m.d);
                let tow = (needs > 0).then(|| at(badge(needs), pet::badge(&m, size), 30.0, 16.0));
                Some(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .children(tow)
                        .child(at(glyph(), spot, GLYPH, GLYPH)),
                )
            }
            PetMode::Idle => {
                let (left, right) = match self.geometry {
                    Some(g) => (g.center_x - g.screen.x, g.screen.max_x() - g.center_x),
                    None => (CANVAS_WIDTH / 2.0, CANVAS_WIDTH / 2.0),
                };
                let edge = perch::edge(left, right, self.model.notch.h);
                let feet = perch::seat(self.perch, self.model.notch.w, edge);
                let centre = Point::new(size.w / 2.0 + feet.x, feet.y - f64::from(GLYPH) / 2.0);
                Some(div().absolute().top_0().left_0().size_full().child(at(
                    glyph(),
                    centre,
                    GLYPH,
                    GLYPH,
                )))
            }
        }
    }
}
