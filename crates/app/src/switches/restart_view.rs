use std::sync::Arc;

use gpui::{div, prelude::*, px, size, AnyView, Context, Render, Window};
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, icon, label, window_appearance, KitStyled};
use notchling_kit::{Icon, Ink, TypeRole};

use super::restart::{turn_back_on, Area, Restart, RestartNotice, Said, TurnOn, NOTICE_BUTTON};

pub const NOTICE_SIZE: (f32, f32) = (460.0, 168.0);
pub const CONFIRM_SIZE: (f32, f32) = (460.0, 300.0);
const NOTE_HEIGHT: f32 = 28.0;

pub fn window_size(confirming: bool, has_note: bool) -> (f32, f32) {
    match (confirming, has_note) {
        (true, _) => CONFIRM_SIZE,
        (false, true) => (NOTICE_SIZE.0, NOTICE_SIZE.1 + NOTE_HEIGHT),
        (false, false) => NOTICE_SIZE,
    }
}

pub struct RestartNoticeView {
    notice: Arc<RestartNotice>,
    turn_on: Option<Arc<dyn TurnOn>>,
    confirming: bool,
    busy: bool,
    note: Option<String>,
}

impl RestartNoticeView {
    pub fn live(notice: Arc<RestartNotice>, turn_on: Arc<dyn TurnOn>) -> Self {
        RestartNoticeView {
            notice,
            turn_on: Some(turn_on),
            confirming: false,
            busy: false,
            note: None,
        }
    }

    pub fn fixed(areas: &[Area], confirming: bool) -> Self {
        let notice = Arc::new(RestartNotice::default());
        for area in areas {
            notice.observe(*area, Said::Off, true);
        }
        RestartNoticeView {
            notice,
            turn_on: None,
            confirming,
            busy: false,
            note: None,
        }
    }

    fn state(&self) -> Restart {
        self.notice.state()
    }

    fn fit(&self, window: &mut Window) {
        if self.turn_on.is_none() {
            return;
        }
        let (w, h) = window_size(self.confirming, self.note.is_some());
        window.resize(size(px(w), px(h)));
    }

    fn ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirming = true;
        self.note = None;
        self.fit(window);
        cx.notify();
    }

    fn not_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirming {
            self.confirming = false;
            self.fit(window);
        } else {
            self.notice.dismiss();
        }
        cx.notify();
    }

    fn confirmed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(turn_on), Some(confirm)) = (self.turn_on.clone(), self.state().confirm()) else {
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        cx.spawn_in(window, async move |this, cx| {
            let failed = cx
                .background_executor()
                .spawn(async move { turn_back_on(&confirm.areas, turn_on.as_ref()) })
                .await;
            this.update_in(cx, |view, window, cx| {
                view.busy = false;
                view.confirming = false;
                view.note = (!failed.is_empty()).then(|| failed.join(" "));
                view.fit(window);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

impl Render for RestartNoticeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let state = self.state();
        let shell = div()
            .id("restart-notice")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(SIDE_PADDING + 4.0))
            .rounded(px(RADIUS_CARD))
            .border(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .fill(Ink::Window, a);
        let rest = if self.busy {
            PartState::Disabled
        } else {
            PartState::Rest
        };
        let cancel_word = if self.confirming { "Cancel" } else { "Not Now" };
        let cancel = button("restart-not-now", cancel_word, ButtonKind::Ghost, rest, a)
            .when(!self.busy, |b| {
                b.on_click(cx.listener(|this, _, window, cx| this.not_now(window, cx)))
            });
        let body = match (self.confirming, state.confirm(), state.banner()) {
            (true, Some(confirm), _) => {
                let ok = button("restart-confirm", confirm.ok, ButtonKind::Primary, rest, a)
                    .when(!self.busy, |b| {
                        b.on_click(cx.listener(|this, _, window, cx| this.confirmed(window, cx)))
                    });
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(label(confirm.title, TypeRole::Title, Ink::Text, a))
                    .children(
                        confirm
                            .lines
                            .iter()
                            .map(|l| label(l, TypeRole::Body, Ink::TextMuted, a)),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.0))
                            .child(cancel)
                            .child(ok),
                    )
            }
            (_, _, Some(banner)) => {
                let open = button(
                    "restart-turn-back-on",
                    NOTICE_BUTTON,
                    ButtonKind::Primary,
                    rest,
                    a,
                )
                .when(!self.busy, |b| {
                    b.on_click(cx.listener(|this, _, window, cx| this.ask(window, cx)))
                });
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(icon(
                                Icon::Permission,
                                TypeRole::Title.spec().size,
                                Ink::Needs,
                                a,
                            ))
                            .child(label(&banner, TypeRole::Heading, Ink::Text, a)),
                    )
                    .child(label(
                        "Notchling tried to turn these back on after it restarted, and couldn't.",
                        TypeRole::Meta,
                        Ink::TextMuted,
                        a,
                    ))
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.0))
                            .child(cancel)
                            .child(open),
                    )
            }
            _ => div().flex_1().child(label(
                "Everything is back on.",
                TypeRole::Body,
                Ink::TextMuted,
                a,
            )),
        };
        shell.child(body).children(
            self.note
                .as_deref()
                .map(|n| label(n, TypeRole::Meta, Ink::Needs, a)),
        )
    }
}

pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    let confirming = match name {
        "restart-notice" => false,
        "restart-notice-confirm" => true,
        _ => return None,
    };
    let areas = [Area::Acting, Area::Vault, Area::Accounts];
    Some(
        cx.new(|_| RestartNoticeView::fixed(&areas, confirming))
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_has_its_own_window_height() {
        let notice = window_size(false, false);
        let confirm = window_size(true, false);
        assert_eq!((notice, confirm), (NOTICE_SIZE, CONFIRM_SIZE));
        assert!(
            notice.1 < confirm.1,
            "the notice is not the confirmation's height"
        );
        assert!(window_size(false, true).1 > notice.1);
        assert_eq!(window_size(true, true), confirm);
        assert_eq!(notice.0, confirm.0);
    }
}
