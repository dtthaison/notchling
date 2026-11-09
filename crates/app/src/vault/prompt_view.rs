//! An agent's request form over GPUI (macOS), in a floating window of its own
//! (`mac::vault_prompts` opens it without taking the focus when the daemon asks, and closes it when
//! the answer is in; closing it is Cancel). Esc cancels; Return never saves an agent's request, so
//! a key pressed in another app can't add a secret. The approval card is `approval_view`. A render
//! has a fixed request and no daemon.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{div, prelude::*, px, Context, FocusHandle, Focusable, KeyDownEvent, Render, Window};
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::ui::{hsla, icon, label, scrollable, window_appearance, KitStyled};
use notchling_kit::{Icon, Ink, TypeRole};

use super::form::{Form, Typed};
use super::form_view::{clipboard_key, form_body, form_footer, FormEvent};
use super::store::Vault;
use super::wire::PendingRequest;
use crate::accounts::view_rows::Handler;
use crate::switches::feed::SwitchFeed;
use crate::switches::Switch;

/// The request window's size in points.
pub const REQUEST_SIZE: (f32, f32) = (540.0, 720.0);

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
            FormEvent::ConfirmHost => self.form.confirm_host(),
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
        if clipboard_key(&mut self.form, k, cx) {
            cx.stop_propagation();
            cx.notify();
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
                    .child(icon(
                        Icon::Permission,
                        TypeRole::Heading.spec().size,
                        Ink::TextMuted,
                        a,
                    ))
                    .child(label(self.form.title(), TypeRole::Heading, Ink::Text, a)),
            )
            .child(scrollable(form_body(&self.form, self.today(), a, cx, on)))
            .child(footer)
            .into_any_element()
    }
}
