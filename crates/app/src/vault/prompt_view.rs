//! The vault's prompt windows over GPUI (macOS): the approval card (Always Allow / Deny / Allow
//! Once, the folder and the command shown) and an agent's request form. Each is its own floating
//! window (`mac::vault_prompts` opens it the moment the daemon asks and closes it when the answer
//! is in). Esc denies or cancels; Return does not allow, so a key pressed in another app a moment
//! before the window appeared can't hand out a secret. A render has a fixed prompt and no daemon.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    div, prelude::*, px, Context, Div, FocusHandle, Focusable, KeyDownEvent, Render, Stateful,
    Window,
};
use notchling_kit::metric::{RADIUS_CARD, RADIUS_MD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, label, window_appearance, KitStyled, MONO_FONT};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::approval::{self, Prompt, ALWAYS, COMMAND, DENY, FOLDER, ONCE, WINDOW_TITLE};
use super::form::{Form, Typed};
use super::form_view::{form_body, form_footer, FormEvent};
use super::store::Vault;
use super::wire::{Decision, PendingRequest};
use crate::accounts::view_rows::Handler;
use crate::mac::gate;
use crate::switches::feed::SwitchFeed;
use crate::switches::Switch;

/// The request window's size in points.
pub const REQUEST_SIZE: (f32, f32) = (540.0, 720.0);

pub struct VaultApprovalView {
    feed: Arc<SwitchFeed>,
    vault: Arc<Vault>,
    fixed: Option<Prompt>,
    busy: bool,
    note: Option<String>,
    focus: FocusHandle,
}

impl VaultApprovalView {
    pub fn live(feed: Arc<SwitchFeed>, vault: Arc<Vault>, cx: &mut Context<Self>) -> Self {
        VaultApprovalView {
            feed,
            vault,
            fixed: None,
            busy: false,
            note: None,
            focus: cx.focus_handle(),
        }
    }

    pub fn fixed(prompt: Prompt, cx: &mut Context<Self>) -> Self {
        VaultApprovalView {
            feed: Arc::new(SwitchFeed::none()),
            vault: Arc::new(Vault::fixed(Vec::new())),
            fixed: Some(prompt),
            busy: false,
            note: None,
            focus: cx.focus_handle(),
        }
    }

    fn shown(&self) -> Option<Prompt> {
        self.fixed
            .clone()
            .or_else(|| approval::prompt(&self.feed.approvals()))
    }

    /// Sends the answer. The prompt is gone at once; one the daemon no longer holds answers
    /// `answered: false`, which is not an error.
    fn answer(&mut self, id: String, decision: Decision, cx: &mut Context<Self>) {
        if self.busy || !self.vault.is_live() {
            return;
        }
        self.busy = true;
        self.note = None;
        self.feed.resolve(&id);
        let (vault, switches) = (self.vault.clone(), self.feed.switches());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { vault.answer(&switches, &id, decision) })
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

fn mono_box(text: &str, a: Appearance) -> Stateful<Div> {
    div()
        .id("command-box")
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

impl Render for VaultApprovalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        if !self.focus.is_focused(window) && self.fixed.is_none() {
            window.focus(&self.focus);
        }
        let Some(prompt) = self.shown() else {
            return div().size_full().fill(Ink::Window, a).into_any_element();
        };
        let live = self.vault.is_live() && self.feed.switches().is_on(Switch::Vault);
        let state = if self.busy || (!live && self.fixed.is_none()) {
            PartState::Disabled
        } else {
            PartState::Rest
        };
        let act = |id: &'static str,
                   title: &str,
                   kind: ButtonKind,
                   decision: Decision,
                   cx: &mut Context<Self>| {
            let request = prompt.id.clone();
            button(id, title, kind, state, a).when(state == PartState::Rest, |b| {
                b.on_click(
                    cx.listener(move |this, _, _, cx| this.answer(request.clone(), decision, cx)),
                )
            })
        };
        let always = gate::tip_text(
            act(
                "vault-always",
                ALWAYS,
                ButtonKind::Ghost,
                Decision::Always,
                cx,
            ),
            prompt.always_tip.clone().into(),
            a,
        );
        let deny = act(
            "vault-deny",
            DENY,
            ButtonKind::Secondary,
            Decision::Deny,
            cx,
        );
        let once = act("vault-once", ONCE, ButtonKind::Primary, Decision::Once, cx);
        let esc = prompt.id.clone();
        div()
            .id("vault-approval")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.answer(esc.clone(), Decision::Deny, cx);
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
            .child(section(
                FOLDER,
                div()
                    .font_family(MONO_FONT)
                    .text_size(px(12.0))
                    .ink(Ink::Text, a)
                    .child(prompt.folder.clone()),
                a,
            ))
            .child(section(COMMAND, mono_box(&prompt.command, a), a))
            .child(label(prompt.explanation, TypeRole::Meta, Ink::TextMuted, a))
            .children(
                prompt
                    .more
                    .as_deref()
                    .map(|more| label(more, TypeRole::Meta, Ink::TextFaint, a)),
            )
            .children(
                self.note
                    .as_deref()
                    .map(|note| label(note, TypeRole::Meta, Ink::Needs, a)),
            )
            .child(div().flex_1())
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
            .into_any_element()
    }
}

/// An agent's request form in a window of its own.
pub struct VaultRequestView {
    feed: Arc<SwitchFeed>,
    vault: Arc<Vault>,
    id: String,
    form: Form,
    busy: bool,
    done: bool,
    focus: FocusHandle,
    fixed_today: Option<i64>,
}

impl VaultRequestView {
    pub fn live(
        feed: Arc<SwitchFeed>,
        vault: Arc<Vault>,
        pending: PendingRequest,
        cx: &mut Context<Self>,
    ) -> Self {
        VaultRequestView {
            feed,
            vault,
            id: pending.id.clone(),
            form: Form::request(&pending),
            busy: false,
            done: false,
            focus: cx.focus_handle(),
            fixed_today: None,
        }
    }

    pub fn fixed(pending: PendingRequest, today: i64, cx: &mut Context<Self>) -> Self {
        VaultRequestView {
            feed: Arc::new(SwitchFeed::fixed(crate::switches::Switches::all_on())),
            vault: Arc::new(Vault::fixed(Vec::new())),
            id: pending.id.clone(),
            form: Form::request(&pending),
            busy: false,
            done: false,
            focus: cx.focus_handle(),
            fixed_today: Some(today),
        }
    }

    fn today(&self) -> i64 {
        self.fixed_today.unwrap_or_else(|| {
            notchling_engine::vault::expiry::today_local(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0),
            )
        })
    }

    fn finish(
        &mut self,
        job: impl FnOnce(&Vault, &crate::switches::Switches) -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.busy || !self.vault.is_live() {
            return;
        }
        self.busy = true;
        let (vault, switches) = (self.vault.clone(), self.feed.switches());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { job(&vault, &switches) })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                match result {
                    Ok(()) => {
                        view.done = true;
                        view.feed.resolve(&view.id);
                    }
                    Err(e) => view.form.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if !self.form.can_save() {
            return;
        }
        let (entry, value, replacing) = match self.form.to_save() {
            Ok(parts) => parts,
            Err(e) => {
                self.form.error = Some(e);
                cx.notify();
                return;
            }
        };
        let request = self.id.clone();
        self.form.forget_value();
        self.finish(
            move |v, sw| {
                v.save(
                    sw,
                    &entry,
                    value.as_ref(),
                    replacing.as_deref(),
                    Some(&request),
                )
            },
            cx,
        );
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        self.finish(move |v, sw| v.cancel_request(sw, &id), cx);
    }

    fn event(&mut self, event: FormEvent, cx: &mut Context<Self>) {
        let today = self.today();
        match event {
            FormEvent::Save => return self.save(cx),
            FormEvent::Cancel => return self.cancel(cx),
            FormEvent::Focus(field) => self.form.focus = field,
            FormEvent::Kind(kind) => self.form.set_kind(kind),
            FormEvent::ToggleAsk => self.form.ask = !self.form.ask,
            FormEvent::ToggleAdvanced => self.form.advanced = !self.form.advanced,
            FormEvent::EditHowTo => self.form.editing_how_to = true,
            FormEvent::NoExpiry(none) => self.form.choose_expiry(none, today),
        }
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if k.key == "escape" {
            self.cancel(cx);
            cx.stop_propagation();
            return;
        }
        let modified = k.modifiers.platform || k.modifiers.control;
        match self.form.key(&k.key, k.key_char.as_deref(), modified) {
            Typed::Submit => self.save(cx),
            Typed::Changed => {}
            Typed::Ignored => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Focusable for VaultRequestView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for VaultRequestView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        if !self.focus.is_focused(window) && self.fixed_today.is_none() {
            window.focus(&self.focus);
        }
        if self.done {
            return div().size_full().fill(Ink::Window, a).into_any_element();
        }
        let on: Handler<Self, FormEvent> = Rc::new(|this, event, cx| this.event(event, cx));
        let off = self.feed.switches().message(Switch::Vault);
        let footer = form_footer(&self.form, self.busy, off, a, cx, on.clone());
        div()
            .id("vault-request")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key(event, cx)))
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .border(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .fill(Ink::Window, a)
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(SIDE_PADDING))
                    .py(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .border_b(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
                    .child(label(
                        Icon::Permission.glyph(),
                        TypeRole::Heading,
                        Ink::TextMuted,
                        a,
                    ))
                    .child(label(self.form.title(), TypeRole::Heading, Ink::Text, a)),
            )
            .child(form_body(&self.form, self.today(), a, cx, on))
            .child(footer)
            .into_any_element()
    }
}
