//! The handover checklist over GPUI (part U11, macOS): a row per resource with its owner, what
//! turning it over means, the steps, and the switch. A switch never acts on the click: it opens a
//! dialog that names the change, and only that dialog's button sends anything. A render has a fixed
//! state and no services.

use std::sync::Arc;
use std::time::Duration;

use gpui::{div, prelude::*, px, AnyView, Context, Div, Render, Window};
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, label, window_appearance, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::facts::{SwiftApp, SwiftProbe};
use super::handover::{confirm, rows, Confirm, Control, Owner, Row, State, INTRO, TITLE};
use super::hooks::Hooks;
use super::renders::{handover_state, pending_confirm, shown, Shown};
use super::switches::Known;
use crate::acting::Acting;
use crate::actions::Actions;
use crate::mac::gate;
use crate::phone::bridge::Bridge;
use crate::phone::view::switch;
use crate::switches::feed::SwitchFeed;
use crate::switches::Switch;

/// How often the live checklist asks the daemon again.
const REFRESH: Duration = Duration::from_secs(3);

/// The services the live checklist calls.
pub struct Services {
    pub actions: Arc<Actions>,
    pub bridge: Arc<Bridge>,
    pub switches: Arc<SwitchFeed>,
    pub hooks: Arc<Hooks>,
    pub probe: Arc<dyn SwiftProbe>,
}

impl Services {
    /// Asks everything once. Blocking.
    fn read(&self) -> State {
        let acting = self.actions.refresh();
        self.bridge.refresh();
        let switches = Known::from_feed(self.switches.switches());
        State {
            swift: self.probe.find(),
            acting,
            bridge: self.bridge.snapshot(),
            switches,
            hook: self.hooks.plan().ok(),
        }
    }

    /// Does what the dialog said. Blocking.
    fn apply(&self, control: Control, on: bool) -> Result<(), String> {
        match control {
            Control::Acting => self.actions.choose(on).map(|_| ()),
            Control::Bridge => self.bridge.choose(on),
            Control::Switch(name) => {
                let switch = Switch::parse(name).ok_or(format!("no switch named {name}"))?;
                self.switches.choose(switch, on).map(|_| ())
            }
        }
    }
}

pub struct HandoverView {
    state: State,
    services: Option<Arc<Services>>,
    pending: Option<Confirm>,
    busy: bool,
    note: Option<String>,
}

impl HandoverView {
    /// The live checklist: reads now, then every few seconds while it is open.
    pub fn live(services: Arc<Services>, cx: &mut Context<Self>) -> HandoverView {
        let view = HandoverView {
            state: State {
                swift: SwiftApp::NotFound,
                acting: Acting::Detached,
                bridge: crate::phone::bridge::Snapshot::detached(),
                switches: Known::Detached,
                hook: None,
            },
            services: Some(services),
            pending: None,
            busy: false,
            note: None,
        };
        cx.spawn(async move |this, cx| loop {
            let asked = this.update(cx, |view, _| view.services.clone());
            let Ok(Some(services)) = asked else {
                break;
            };
            let state = cx
                .background_executor()
                .spawn(async move { services.read() })
                .await;
            if this
                .update(cx, |view, cx| {
                    view.state = state;
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
            cx.background_executor().timer(REFRESH).await;
        })
        .detach();
        view
    }

    /// A render's: a fixed state, and a dialog already open when asked.
    pub fn fixed(state: State, pending: Option<Confirm>) -> HandoverView {
        HandoverView {
            state,
            services: None,
            pending,
            busy: false,
            note: None,
        }
    }

    fn ask(&mut self, control: Control, on: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.note = None;
        self.pending = Some(confirm(control, on));
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.pending = None;
        cx.notify();
    }

    fn confirmed(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.pending.take() else {
            return;
        };
        let Some(services) = self.services.clone() else {
            return;
        };
        self.busy = true;
        cx.spawn(async move |this, cx| {
            let (result, state) = cx
                .background_executor()
                .spawn(async move {
                    let result = services.apply(dialog.control, dialog.turn_on);
                    (result, services.read())
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                view.note = result.err();
                view.state = state;
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn owner_pill(owner: Owner, a: Appearance) -> Div {
        let (fill, ink) = match owner {
            Owner::Notchling => (Ink::AccentWash, Ink::AccentText),
            Owner::Swift => (Ink::Raised, Ink::Text),
            Owner::Nobody | Owner::Unknown => (Ink::Raised, Ink::TextMuted),
        };
        div()
            .h(px(20.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .rounded_full()
            .fill(fill, a)
            .child(label(owner.word(), TypeRole::Label, ink, a))
    }

    fn row_card(&self, row: &Row, a: Appearance, cx: &mut Context<Self>) -> Div {
        let steps = row.steps.iter().enumerate().map(|(i, step)| {
            div()
                .flex()
                .gap(px(8.0))
                .child(label(&format!("{}.", i + 1), TypeRole::Meta, Ink::TextFaint, a).w(px(16.0)))
                .child(
                    label(step, TypeRole::Meta, Ink::TextMuted, a)
                        .flex_1()
                        .min_w(px(0.0)),
                )
        });
        let toggle = row.toggle.as_ref().map(|t| {
            let (control, on) = (t.control, t.on);
            let live = t.enabled && !self.busy && self.services.is_some();
            let control_state = crate::acting::Control {
                enabled: live,
                tooltip: t.why,
            };
            let element = gate::tip(switch(row.id, on, live, a), control_state, a);
            element.when(live, |s| {
                s.on_click(cx.listener(move |this, _, _, cx| this.ask(control, !on, cx)))
            })
        });
        card(a)
            .p(px(16.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(label(row.title, TypeRole::Heading, Ink::Text, a))
                    .child(Self::owner_pill(row.owner, a))
                    .child(div().flex_1())
                    .children(toggle),
            )
            .child(label(&row.status, TypeRole::Body, Ink::Text, a))
            .child(label(row.meaning, TypeRole::Meta, Ink::TextMuted, a))
            .child(
                div()
                    .pt(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(label("Steps", TypeRole::Section, Ink::TextFaint, a))
                    .children(steps),
            )
    }

    fn dialog(&self, c: &Confirm, a: Appearance, cx: &mut Context<Self>) -> Div {
        let cancel = button(
            "handover-cancel",
            "Cancel",
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| this.cancel(cx)));
        let ok = button("handover-ok", c.ok, ButtonKind::Primary, PartState::Rest, a)
            .on_click(cx.listener(|this, _, _, cx| this.confirmed(cx)));
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            // The backdrop takes every click: no switch under the dialog is live.
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(Ink::Shadow.color(a)))
            .child(
                card(a)
                    .w(px(460.0))
                    .p(px(22.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(label(&c.title, TypeRole::Title, Ink::Text, a))
                    .children(
                        c.lines
                            .iter()
                            .map(|l| label(l, TypeRole::Body, Ink::TextMuted, a)),
                    )
                    .child(
                        div()
                            .pt(px(6.0))
                            .flex()
                            .justify_end()
                            .gap(px(8.0))
                            .child(cancel)
                            .child(ok),
                    ),
            )
    }
}

impl Render for HandoverView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let cards: Vec<Div> = rows(&self.state)
            .iter()
            .map(|r| self.row_card(r, a, cx))
            .collect();
        let note = self.note.clone();
        let dialog = self.pending.clone().map(|c| self.dialog(&c, a, cx));
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .fill(Ink::Window, a)
            .child(
                div()
                    .id("handover-body")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .p(px(24.0))
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .child(label(TITLE, TypeRole::Title, Ink::Text, a))
                    .child(label(INTRO, TypeRole::Body, Ink::TextMuted, a))
                    .children(cards),
            )
            .children(note.map(|n| {
                label(&n, TypeRole::Meta, Ink::Needs, a)
                    .px(px(SIDE_PADDING + 8.0))
                    .py(px(10.0))
                    .border_t(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
            }))
            .children(dialog)
    }
}

/// `--render handover|handover-confirm`, over a made-up state; nothing is called.
pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    let which = shown(name)?;
    let pending = (which == Shown::HandoverConfirm).then(pending_confirm);
    Some(
        cx.new(|_| HandoverView::fixed(handover_state(), pending))
            .into(),
    )
}
