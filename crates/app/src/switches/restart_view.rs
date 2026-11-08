//! The restart notice over GPUI (macOS): "Notchling restarted: Sessions, Vault are off" with Turn
//! Back On, which opens one confirmation naming each switch; only that confirmation's button turns
//! anything on. `mac::restart_notice` opens it in a small window that doesn't take the focus. A
//! render has a fixed notice and turns nothing on.

use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyView, Context, Render, Window};
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, label, window_appearance, KitStyled};
use notchling_kit::{Icon, Ink, TypeRole};

use super::restart::{turn_back_on, Area, Restart, RestartNotice, Said, TurnOn, NOTICE_BUTTON};

/// The notice window's size in points.
pub const NOTICE_SIZE: (f32, f32) = (460.0, 300.0);

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

    /// A render's: a notice for `areas`, the confirmation open when asked; nothing is sent.
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

    fn ask(&mut self, cx: &mut Context<Self>) {
        self.confirming = true;
        self.note = None;
        cx.notify();
    }

    fn not_now(&mut self, cx: &mut Context<Self>) {
        if self.confirming {
            self.confirming = false;
        } else {
            self.notice.dismiss();
        }
        cx.notify();
    }

    /// The person confirmed: exactly the switches the confirmation named, each through its op.
    fn confirmed(&mut self, cx: &mut Context<Self>) {
        let (Some(turn_on), Some(confirm)) = (self.turn_on.clone(), self.state().confirm()) else {
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        cx.spawn(async move |this, cx| {
            let failed = cx
                .background_executor()
                .spawn(async move { turn_back_on(&confirm.areas, turn_on.as_ref()) })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                view.confirming = false;
                view.note = (!failed.is_empty()).then(|| failed.join(" "));
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
                b.on_click(cx.listener(|this, _, _, cx| this.not_now(cx)))
            });
        let body = match (self.confirming, state.confirm(), state.banner()) {
            (true, Some(confirm), _) => {
                let ok = button("restart-confirm", confirm.ok, ButtonKind::Primary, rest, a)
                    .when(!self.busy, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| this.confirmed(cx)))
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
                    b.on_click(cx.listener(|this, _, _, cx| this.ask(cx)))
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
                            .child(label(
                                Icon::Permission.glyph(),
                                TypeRole::Title,
                                Ink::Needs,
                                a,
                            ))
                            .child(label(&banner, TypeRole::Heading, Ink::Text, a)),
                    )
                    .child(label(
                        "The daemon starts with every switch off, and this app never turns one back on by itself.",
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

/// `--render restart-notice|restart-notice-confirm`, over a made-up restart; nothing is sent.
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
