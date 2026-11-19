use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, Context, Div, FocusHandle, Focusable, KeyDownEvent, MouseButton, Render,
    SharedString, Stateful, Subscription, Window,
};
use notchling_kit::metric::{RADIUS_CARD, RADIUS_MD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, hsla, icon, label, scrollable, window_appearance, KitStyled, MONO_FONT,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::approval::{ApprovalModel, ALWAYS, COMMAND, DENY, FOLDER, ONCE, WINDOW_TITLE};
use super::presence::{Done, Presence};
use super::store::Vault;
use super::wire::{Approval, Decision};
use crate::mac::gate;
use crate::sheets::header::sheet_header_closing;
use crate::switches::feed::SwitchFeed;
use crate::switches::Switch;

pub fn now_ms() -> i64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as i64
}

pub struct VaultApprovalView {
    feed: Arc<SwitchFeed>,
    vault: Arc<Vault>,
    model: ApprovalModel,
    fixed: bool,
    presence: Presence,
    visible: bool,
    active: bool,
    reset_focus: bool,
    focus: FocusHandle,
    _activation: Option<Subscription>,
}

impl gpui::EventEmitter<Done> for VaultApprovalView {}

impl VaultApprovalView {
    pub fn live(
        approval: &Approval,
        feed: Arc<SwitchFeed>,
        vault: Arc<Vault>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::live_in(approval, feed, vault, Presence::Window, window, cx)
    }

    pub fn live_in(
        approval: &Approval,
        feed: Arc<SwitchFeed>,
        vault: Arc<Vault>,
        presence: Presence,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let model = ApprovalModel::new(approval);
        let activation = cx.observe_window_activation(window, |view, window, cx| {
            view.active = window.is_window_active();
            view.refresh_arming(cx);
        });
        let mut view = VaultApprovalView {
            feed,
            vault,
            model,
            fixed: false,
            presence,
            visible: !presence.embedded(),
            active: window.is_window_active(),
            reset_focus: false,
            focus: cx.focus_handle(),
            _activation: Some(activation),
        };
        view.refresh_arming(cx);
        view
    }

    pub fn fixed(approval: &Approval, cx: &mut Context<Self>) -> Self {
        Self::fixed_in(approval, Presence::Window, cx)
    }

    pub fn fixed_in(approval: &Approval, presence: Presence, cx: &mut Context<Self>) -> Self {
        let mut model = ApprovalModel::new(approval);
        model.shown(-super::approval::ARMING_MS);
        VaultApprovalView {
            feed: Arc::new(SwitchFeed::none()),
            vault: Arc::new(Vault::fixed(Vec::new())),
            model,
            fixed: true,
            presence,
            visible: true,
            active: true,
            reset_focus: false,
            focus: cx.focus_handle(),
            _activation: None,
        }
    }

    pub fn set_presence(&mut self, presence: Presence, cx: &mut Context<Self>) {
        if self.presence == presence {
            return;
        }
        self.presence = presence;
        self.reset_focus = true;
        if !self.fixed {
            self.model.rearm(false, 0);
        }
        self.refresh_arming(cx);
        cx.notify();
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.visible = visible;
        self.refresh_arming(cx);
    }

    fn refresh_arming(&mut self, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        let exposed = self.visible && self.active;
        let changed = self.model.expose(exposed, now_ms());
        if changed {
            if exposed {
                self.redraw_when_armed(cx);
            }
            cx.notify();
        }
    }

    pub fn id(&self) -> &str {
        &self.model.prompt.id
    }

    pub fn resolved(&self) -> bool {
        self.model.resolved
    }

    fn now(&self) -> i64 {
        if self.fixed {
            0
        } else {
            now_ms()
        }
    }

    fn redraw_when_armed(&self, cx: &mut Context<Self>) {
        let Some(at) = self.model.arms_at() else {
            return;
        };
        let wait = (at - now_ms()).max(0) as u64 + 20;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(wait))
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    fn answer(&mut self, decision: Decision, cx: &mut Context<Self>) {
        if self.fixed || !self.vault.is_live() {
            return;
        }
        let Some((id, decision)) = self.model.answer(decision, now_ms()) else {
            return;
        };
        let (vault, switches) = (self.vault.clone(), self.feed.switches());
        cx.spawn(async move |this, cx| {
            let sent = id.clone();
            let result = cx
                .background_executor()
                .spawn(async move { vault.answer(&switches, &sent, decision) })
                .await;
            this.update(cx, |view, cx| {
                if result.is_ok() {
                    view.feed.resolve(&id);
                }
                view.model.finished(result);
                if view.model.resolved {
                    cx.emit(Done);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

impl Focusable for VaultApprovalView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

fn section(title: &str, body: impl IntoElement, a: Appearance) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(label(title, TypeRole::Section, Ink::TextFaint, a))
        .child(body)
}

fn mono_box(id: String, text: &str, a: Appearance) -> Stateful<Div> {
    div()
        .id(SharedString::from(id))
        .max_h(px(120.0))
        .overflow_y_scroll()
        .p(px(8.0))
        .rounded(px(RADIUS_MD))
        .fill(Ink::Card, a)
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .font_family(MONO_FONT)
        .text_size(px(12.0))
        .ink(Ink::Text, a)
        .child(text.to_owned())
}

fn mono(text: &str, ink: Ink, a: Appearance) -> Div {
    div()
        .font_family(MONO_FONT)
        .text_size(px(12.0))
        .ink(ink, a)
        .child(text.to_owned())
}

impl Render for VaultApprovalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        if self.reset_focus {
            self.reset_focus = false;
            if self.focus.is_focused(window) {
                window.blur();
            }
        }
        if !self.fixed
            && self.presence.takes_focus()
            && self.visible
            && !self.focus.is_focused(window)
        {
            window.focus(&self.focus);
        }
        let prompt = self.model.prompt.clone();
        let switched_on =
            self.fixed || (self.vault.is_live() && self.feed.switches().is_on(Switch::Vault));
        let now = self.now();
        let act = |part: &str, title: &str, kind: ButtonKind, decision: Decision| {
            let live = switched_on && self.model.live(decision, now);
            let state = if live {
                PartState::Rest
            } else {
                PartState::Disabled
            };
            button(
                SharedString::from(prompt.element(part)),
                title,
                kind,
                state,
                a,
            )
            .when(live && !self.fixed, |b| {
                b.on_click(cx.listener(move |this, _, _, cx| this.answer(decision, cx)))
            })
        };
        let always = gate::tip_text(
            act("vault-always", ALWAYS, ButtonKind::Ghost, Decision::Always),
            prompt.always_tip.clone().into(),
            a,
        );
        let deny = act("vault-deny", DENY, ButtonKind::Secondary, Decision::Deny);
        let once = act("vault-once", ONCE, ButtonKind::Primary, Decision::Once);
        let folder = div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(mono(&prompt.folder, Ink::Text, a))
            .children(
                prompt
                    .claimed_folder
                    .as_deref()
                    .map(|c| mono(c, Ink::TextMuted, a)),
            )
            .children(
                prompt
                    .unchecked
                    .map(|u| label(u, TypeRole::Meta, Ink::TextMuted, a)),
            );
        let title = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(icon(
                Icon::Permission,
                TypeRole::Title.spec().size,
                Ink::AccentText,
                a,
            ))
            .child(label(&prompt.title, TypeRole::Title, Ink::Text, a));
        let details = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .children(
                prompt
                    .requested_by
                    .as_deref()
                    .map(|r| label(r, TypeRole::Meta, Ink::TextMuted, a)),
            )
            .children((!prompt.folder.is_empty()).then(|| section(FOLDER, folder, a)))
            .children((!prompt.command.is_empty()).then(|| {
                section(
                    COMMAND,
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(scrollable(mono_box(
                            prompt.element("command-box"),
                            &prompt.command,
                            a,
                        )))
                        .children(
                            prompt
                                .command_size
                                .as_deref()
                                .map(|s| label(s, TypeRole::Meta, Ink::Needs, a)),
                        ),
                    a,
                )
            }))
            .child(label(prompt.explanation, TypeRole::Meta, Ink::TextMuted, a))
            .children(
                self.model
                    .note
                    .as_deref()
                    .map(|note| label(note, TypeRole::Meta, Ink::Needs, a)),
            );
        let tail = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .children(
                prompt
                    .warning
                    .map(|w| label(w, TypeRole::Body, Ink::Expired, a)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(always)
                    .child(div().flex_1())
                    .child(deny)
                    .child(once),
            );
        let frame = div()
            .id(SharedString::from(prompt.element("vault-approval")))
            .track_focus(&self.focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, _| window.focus(&this.focus)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.answer(Decision::Deny, cx);
                    cx.stop_propagation();
                }
            }));
        match self.presence {
            Presence::Window => frame
                .size_full()
                .flex()
                .flex_col()
                .gap(px(12.0))
                .p(px(SIDE_PADDING + 4.0))
                .rounded(px(RADIUS_CARD))
                .border(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
                .fill(Ink::Window, a)
                .child(label(WINDOW_TITLE, TypeRole::Section, Ink::TextFaint, a))
                .child(title)
                .child(details)
                .child(div().flex_1())
                .child(tail),
            Presence::Card => frame
                .flex()
                .flex_col()
                .gap(px(12.0))
                .p(px(14.0))
                .child(title)
                .child(details)
                .child(tail),
            Presence::Sheet => frame
                .size_full()
                .flex()
                .flex_col()
                .child(sheet_header_closing(
                    Icon::Permission,
                    WINDOW_TITLE,
                    Vec::new(),
                    a,
                    cx,
                    |this: &mut Self, _, cx| this.answer(Decision::Deny, cx),
                ))
                .child(scrollable(
                    div()
                        .id(SharedString::from(prompt.element("vault-approval-body")))
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .p(px(SIDE_PADDING + 4.0))
                        .child(title)
                        .child(details),
                ))
                .child(
                    div()
                        .flex_shrink_0()
                        .px(px(SIDE_PADDING + 4.0))
                        .pb(px(SIDE_PADDING + 4.0))
                        .child(tail),
                ),
        }
    }
}
