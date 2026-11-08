//! The vault approval card over GPUI (macOS): Always Allow / Deny / Allow Once, who asked, the
//! folder and the command. One window per approval (`mac::vault_prompts`), and the view is bound to
//! that approval for its life: it never reads the feed for another one. Allow arms
//! [`super::approval::ARMING_MS`] after the window appears or becomes key; every button's id names
//! the approval; the prompt resolves only once the daemon took the answer. Esc denies; Return does
//! nothing. A render has a fixed approval and no daemon.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, Context, Div, FocusHandle, Focusable, KeyDownEvent, Render, SharedString,
    Stateful, Subscription, Window,
};
use notchling_kit::metric::{RADIUS_CARD, RADIUS_MD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, label, window_appearance, KitStyled, MONO_FONT};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::approval::{ApprovalModel, ALWAYS, COMMAND, DENY, FOLDER, ONCE, WINDOW_TITLE};
use super::store::Vault;
use super::wire::{Approval, Decision};
use crate::mac::gate;
use crate::switches::feed::SwitchFeed;
use crate::switches::Switch;

/// Milliseconds on a clock that only goes forward.
pub fn now_ms() -> i64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as i64
}

pub struct VaultApprovalView {
    feed: Arc<SwitchFeed>,
    vault: Arc<Vault>,
    model: ApprovalModel,
    /// A render's: drawn armed, never sends.
    fixed: bool,
    focus: FocusHandle,
    _activation: Option<Subscription>,
}

impl VaultApprovalView {
    /// The live card for `approval`, in `window`. It arms when the window becomes key.
    pub fn live(
        approval: &Approval,
        feed: Arc<SwitchFeed>,
        vault: Arc<Vault>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut model = ApprovalModel::new(approval);
        model.shown(now_ms());
        let activation = cx.observe_window_activation(window, |view, window, cx| {
            if window.is_window_active() {
                view.model.shown(now_ms());
                view.redraw_when_armed(cx);
            } else {
                view.model.hidden();
            }
            cx.notify();
        });
        let view = VaultApprovalView {
            feed,
            vault,
            model,
            fixed: false,
            focus: cx.focus_handle(),
            _activation: Some(activation),
        };
        view.redraw_when_armed(cx);
        view
    }

    pub fn fixed(approval: &Approval, cx: &mut Context<Self>) -> Self {
        let mut model = ApprovalModel::new(approval);
        model.shown(-super::approval::ARMING_MS);
        VaultApprovalView {
            feed: Arc::new(SwitchFeed::none()),
            vault: Arc::new(Vault::fixed(Vec::new())),
            model,
            fixed: true,
            focus: cx.focus_handle(),
            _activation: None,
        }
    }

    /// The approval this window answers.
    pub fn id(&self) -> &str {
        &self.model.prompt.id
    }

    /// Whether the daemon took an answer, so the window can go.
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

    /// Draws again when the Allow buttons arm, so they show live.
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

    /// A click: sent only while that button is live; resolved only once the daemon takes it.
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
        if !self.fixed && !self.focus.is_focused(window) {
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
            );
        div()
            .id(SharedString::from(prompt.element("vault-approval")))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.answer(Decision::Deny, cx);
                    cx.stop_propagation();
                }
            }))
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(label(
                        Icon::Permission.glyph(),
                        TypeRole::Title,
                        Ink::AccentText,
                        a,
                    ))
                    .child(label(&prompt.title, TypeRole::Title, Ink::Text, a)),
            )
            .children(
                prompt
                    .requested_by
                    .as_deref()
                    .map(|r| label(r, TypeRole::Meta, Ink::TextMuted, a)),
            )
            .child(section(FOLDER, folder, a))
            .child(section(
                COMMAND,
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(mono_box(prompt.element("command-box"), &prompt.command, a))
                    .children(
                        prompt
                            .command_size
                            .as_deref()
                            .map(|s| label(s, TypeRole::Meta, Ink::Needs, a)),
                    ),
                a,
            ))
            .child(label(prompt.explanation, TypeRole::Meta, Ink::TextMuted, a))
            .children(
                self.model
                    .note
                    .as_deref()
                    .map(|note| label(note, TypeRole::Meta, Ink::Needs, a)),
            )
            .child(div().flex_1())
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
            )
    }
}
