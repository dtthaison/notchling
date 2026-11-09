//! The Phone panel over GPUI (part U6, macOS): a card with the bridge switch, the pairing QR and
//! code, the paired phones and the tunnel. Opened from the Phone button in Chat's header
//! (`chat::view::phone`), drawn over the window like a popover. Every call to the daemon runs on
//! the background executor; a control the bridge doesn't allow is drawn disabled with its reason
//! on hover (`mac::gate`). A render has a fixed snapshot and clock and never calls anything.

use std::sync::Arc;
use std::time::Duration;

use gpui::{div, prelude::*, px, AnyView, Context, Div, Render, SharedString, Stateful, Window};
use notchling_kit::metric::{RADIUS_CARD, RADIUS_SM, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, hsla, label, phone_mark, scrollable, window_appearance, KitStyled, MONO_FONT,
};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::bridge::{now_ms, Bridge};
use super::panel::{
    Devices, Dot, Pair, Panel, StatusLine, TunnelBlock, SWITCH_NOTE, TUNNEL_ORIGIN,
};
use super::qr::{Qr, QUIET_ZONE};
use super::renders::{shown, snapshot, NOW_MS};
use crate::acting::Control;
use crate::mac::gate;

/// The panel's width; its height is the window's.
pub const WIDTH: f32 = 400.0;
/// How big the QR is drawn, at most.
const QR_SIZE: f32 = 150.0;
/// How often the panel asks the daemon while it is open.
const REFRESH_EVERY: u32 = 2;

pub struct PhonePanelView {
    bridge: Arc<Bridge>,
    /// A render's clock; the live one otherwise.
    fixed_now: Option<i64>,
    /// A call is out: the controls wait for it.
    busy: bool,
    /// The last thing a click failed at, until the next click.
    note: Option<String>,
}

impl PhonePanelView {
    /// The live panel over the app's bridge: reads it now, then every couple of seconds while it
    /// is on screen (dropping the view ends the loop).
    pub fn live(bridge: Arc<Bridge>, cx: &mut Context<Self>) -> PhonePanelView {
        let view = PhonePanelView {
            bridge,
            fixed_now: None,
            busy: false,
            note: None,
        };
        view.refresh(cx);
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                ticks = ticks.wrapping_add(1);
                // The countdown moves every second; the daemon is asked every few.
                let asking = this.update(cx, |view, cx| {
                    cx.notify();
                    ticks
                        .is_multiple_of(REFRESH_EVERY)
                        .then(|| view.bridge.clone())
                });
                match asking {
                    Ok(Some(bridge)) => {
                        cx.background_executor()
                            .spawn(async move { bridge.refresh() })
                            .await;
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        })
        .detach();
        view
    }

    /// A render's: a fixed snapshot at a fixed time, with nothing behind it.
    pub fn fixed(bridge: Arc<Bridge>) -> PhonePanelView {
        PhonePanelView {
            bridge,
            fixed_now: Some(NOW_MS),
            busy: false,
            note: None,
        }
    }

    fn now(&self) -> i64 {
        self.fixed_now.unwrap_or_else(now_ms)
    }

    fn refresh(&self, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move { bridge.refresh() })
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    /// Runs `job` off the UI thread, then reads the daemon again; an error is shown as the note.
    fn run(
        &mut self,
        job: impl FnOnce(&Bridge) -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.busy || !self.bridge.is_live() {
            return;
        }
        self.busy = true;
        self.note = None;
        let bridge = self.bridge.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let result = job(&bridge);
                    bridge.refresh();
                    result
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                view.note = result.err();
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn toggle(&mut self, on: bool, cx: &mut Context<Self>) {
        self.run(move |b| b.choose(on), cx);
    }

    fn pair(&mut self, cx: &mut Context<Self>) {
        self.run(|b| b.begin_pairing(), cx);
    }

    fn revoke(&mut self, id: String, cx: &mut Context<Self>) {
        self.run(move |b| b.revoke(&id), cx);
    }

    fn close_code(&mut self, cx: &mut Context<Self>) {
        self.bridge.close_pairing();
        cx.notify();
    }
}

fn dot_ink(dot: Dot) -> Ink {
    match dot {
        Dot::On => Ink::Done,
        Dot::Error => Ink::Needs,
        Dot::Off => Ink::TextFaint,
    }
}

fn dot(d: Dot, a: Appearance) -> Div {
    div()
        .size(px(7.0))
        .flex_shrink_0()
        .rounded_full()
        .fill(dot_ink(d), a)
}

fn section_title(text: &str, a: Appearance) -> Div {
    label(text, TypeRole::Section, Ink::TextFaint, a)
}

/// A switch: a capsule with its knob at the end it's set to; dim and plain-cursored while disabled.
pub(crate) fn switch(id: &'static str, on: bool, enabled: bool, a: Appearance) -> Stateful<Div> {
    let knob = div().size(px(14.0)).rounded_full().bg(gpui::white());
    let base = div()
        .id(id)
        .w(px(32.0))
        .h(px(18.0))
        .flex_shrink_0()
        .p(px(2.0))
        .rounded_full()
        .flex()
        .when(on, |s| s.justify_end())
        .fill(if on { Ink::Accent } else { Ink::Raised }, a)
        .child(knob);
    if enabled {
        base.cursor_pointer()
    } else {
        base.opacity(0.45)
    }
}

/// The code as bars on a white card with its quiet zone.
fn qr_view(qr: &Qr) -> Div {
    let modules = qr.width() + 2 * QUIET_ZONE;
    // Whole points per module, so neighbouring bars never leave a seam.
    let cell = (QR_SIZE / modules as f32).floor().max(2.0);
    let mut grid = div()
        .relative()
        .flex_shrink_0()
        .size(px(cell * modules as f32))
        .rounded(px(RADIUS_SM))
        .bg(gpui::white());
    for y in 0..qr.width() {
        for (start, len) in qr.runs(y) {
            grid = grid.child(
                div()
                    .absolute()
                    .top(px((y + QUIET_ZONE) as f32 * cell))
                    .left(px((start + QUIET_ZONE) as f32 * cell))
                    .w(px(len as f32 * cell))
                    .h(px(cell))
                    .bg(gpui::black()),
            );
        }
    }
    grid
}

impl PhonePanelView {
    fn header(&self, status: &StatusLine, a: Appearance) -> Div {
        div()
            .flex_shrink_0()
            .px(px(SIDE_PADDING))
            .py(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .border_b(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(phone_mark(16.0, Ink::TextMuted, a))
            .child(label("Phone", TypeRole::Title, Ink::Text, a))
            .child(div().flex_1())
            .child(dot(status.dot, a))
            .child(label(&status.word, TypeRole::Meta, Ink::TextMuted, a).elide())
    }

    fn switch_section(&self, panel: &Panel, a: Appearance, cx: &mut Context<Self>) -> Div {
        let control = panel.switch.control;
        let on = panel.switch.on;
        let switch = gate::tip(switch("bridge-switch", on, control.enabled, a), control, a)
            .when(control.enabled && !self.busy, |s| {
                s.on_click(cx.listener(move |this, _, _, cx| this.toggle(!on, cx)))
            });
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(label("Let phones connect", TypeRole::Heading, Ink::Text, a).flex_1())
                    .child(switch),
            )
            .child(label(SWITCH_NOTE, TypeRole::Meta, Ink::TextMuted, a))
    }

    fn action(
        &self,
        id: impl Into<gpui::ElementId>,
        title: &str,
        kind: ButtonKind,
        control: Control,
        a: Appearance,
    ) -> Stateful<Div> {
        let state = if self.busy && control.enabled {
            PartState::Disabled
        } else {
            gate::state(control)
        };
        gate::tip(button(id, title, kind, state, a), control, a)
    }

    fn pair_section(&self, pair: &Pair, a: Appearance, cx: &mut Context<Self>) -> Div {
        let body = match pair {
            Pair::Idle { button } => {
                let pair_button = self
                    .action(
                        "pair-phone",
                        "Pair a Phone",
                        ButtonKind::Primary,
                        *button,
                        a,
                    )
                    .when(button.enabled && !self.busy, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| this.pair(cx)))
                    });
                div().flex().child(pair_button)
            }
            Pair::Showing {
                qr,
                code,
                expires,
                expired,
                hosts,
                new_code,
            } => {
                let picture = match qr {
                    Ok(qr) => qr_view(qr),
                    Err(why) => {
                        div()
                            .w(px(QR_SIZE))
                            .child(label(why, TypeRole::Meta, Ink::Needs, a))
                    }
                };
                let new_button = self
                    .action("new-code", "New Code", ButtonKind::Secondary, *new_code, a)
                    .when(new_code.enabled && !self.busy, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| this.pair(cx)))
                    });
                let close = button("close-code", "Close", ButtonKind::Ghost, PartState::Rest, a)
                    .on_click(cx.listener(|this, _, _, cx| this.close_code(cx)));
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(14.0))
                            .child(picture)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.0))
                                    .child(label(
                                        "Scan with the phone app, or type the code:",
                                        TypeRole::Meta,
                                        Ink::TextMuted,
                                        a,
                                    ))
                                    .child(
                                        div()
                                            .font_family(MONO_FONT)
                                            .text_size(px(20.0))
                                            .ink(Ink::Text, a)
                                            .child(code.clone()),
                                    )
                                    .child(label(
                                        expires,
                                        TypeRole::Meta,
                                        if *expired { Ink::Needs } else { Ink::TextMuted },
                                        a,
                                    ))
                                    .child(
                                        div().flex().gap(px(8.0)).child(new_button).child(close),
                                    ),
                            ),
                    )
                    .when(!hosts.is_empty(), |c| {
                        c.child(label(&hosts.join("\n"), TypeRole::Mono, Ink::TextFaint, a))
                    })
            }
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(section_title("Pair", a))
            .child(body)
    }

    fn devices_section(&self, devices: &Devices, a: Appearance, cx: &mut Context<Self>) -> Div {
        let rows = devices.rows.iter().map(|row| {
            let id = row.id.clone();
            let revoke = self
                .action(
                    SharedString::from(format!("revoke-{}", row.id)),
                    "Revoke",
                    ButtonKind::Secondary,
                    row.revoke,
                    a,
                )
                .when(row.revoke.enabled && !self.busy, |b| {
                    b.on_click(cx.listener(move |this, _, _, cx| this.revoke(id.clone(), cx)))
                });
            card(a)
                .p(px(10.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(phone_mark(16.0, Ink::TextMuted, a))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(label(&row.name, TypeRole::Heading, Ink::Text, a).elide())
                        .child(label(&row.detail, TypeRole::Meta, Ink::TextMuted, a)),
                )
                .child(revoke)
        });
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(section_title(&devices.title, a))
            .children(
                devices
                    .empty
                    .map(|empty| label(empty, TypeRole::Meta, Ink::TextMuted, a)),
            )
            .children(rows)
    }

    fn tunnel_section(&self, tunnel: &TunnelBlock, a: Appearance) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(section_title("Internet", a))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(dot(tunnel.dot, a))
                    .child(label(&tunnel.status, TypeRole::Body, Ink::Text, a)),
            )
            .children(
                tunnel
                    .detail
                    .as_deref()
                    .map(|d| label(d, TypeRole::Meta, Ink::TextMuted, a)),
            )
            .child(label(TUNNEL_ORIGIN, TypeRole::Mono, Ink::TextFaint, a))
    }
}

impl Render for PhonePanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let panel = Panel::new(&self.bridge.snapshot(), self.now());
        let error = self.note.clone().or_else(|| panel.error.clone());
        let mut body = div()
            .id("phone-body")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p(px(SIDE_PADDING))
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(self.switch_section(&panel, a, cx))
            .child(self.pair_section(&panel.pair, a, cx))
            .child(self.devices_section(&panel.devices, a, cx))
            .child(self.tunnel_section(&panel.tunnel, a));
        if let Some(fingerprint) = &panel.fingerprint {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(section_title("Certificate", a))
                    .child(label(
                        "SHA-256 fingerprint",
                        TypeRole::Meta,
                        Ink::TextMuted,
                        a,
                    ))
                    .child(label(fingerprint, TypeRole::Mono, Ink::Text, a)),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .overflow_hidden()
            .border(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .fill(Ink::Window, a)
            .child(self.header(&panel.status, a))
            .child(scrollable(body))
            .children(error.map(|e| {
                label(&e, TypeRole::Meta, Ink::Needs, a)
                    .px(px(SIDE_PADDING))
                    .py(px(10.0))
                    .border_t(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
            }))
    }
}

/// `--render phone-panel[-off|-unpaired|-tunnel|-detached]`, over made-up data; nothing is called.
pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    let which = shown(name)?;
    let bridge = Arc::new(Bridge::fixed(snapshot(which)));
    Some(cx.new(|_| PhonePanelView::fixed(bridge)).into())
}
