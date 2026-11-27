use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, Context, FocusHandle, Focusable, KeyDownEvent, MouseButton, Render,
    Subscription, Window,
};
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::ui::{
    hsla, icon, label, parts_on, scrollable, set_parts, window_appearance, KitStyled,
};
use notchling_kit::{Icon, Ink, TypeRole};

use super::approval::requested_by;
use super::closing;
use super::consent_save::{Failure, Flow};
use super::copy_name::{self, Copied};
use super::form::{copyable, Enter, Form};
use super::form_view::{form_body, form_footer, Editing, FormEditors, FormEvent};
use super::lock_watch;
use super::presence::{escape_act, Done, EscapeAct, Presence};
use super::reveal::{FormShown, Seen};
use super::reveal_view::notify_when_hidden;
use super::store::Vault;
use super::twins::{Cover, Stage};
use super::wire::PendingRequest;
use crate::accounts::view_rows::Handler;
use crate::sheets::header::{close_button, sheet_header_closing};
use crate::switches::feed::SwitchFeed;
use crate::switches::Switch;

pub const REQUEST_SIZE: (f32, f32) = (540.0, 720.0);
pub const CARD_BODY_MAX: f32 = 240.0;

pub struct VaultRequestView {
    feed: Arc<SwitchFeed>,
    vault: Arc<Vault>,
    id: String,
    cover: Cover,
    form: Form,
    editors: FormEditors,
    busy: bool,
    flow: Flow,
    presence: Presence,
    visible: bool,
    typing: bool,
    reset_focus: bool,
    want_focus: bool,
    requested_by: Option<String>,
    focus: FocusHandle,
    fixed_today: Option<i64>,
    copied: Copied,
    secret_shown: FormShown,
    seen_lock: Seen,
    _watching: Vec<Subscription>,
}

impl gpui::EventEmitter<Done> for VaultRequestView {}

impl VaultRequestView {
    pub fn live(
        feed: Arc<SwitchFeed>,
        vault: Arc<Vault>,
        pending: PendingRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::live_in(feed, vault, pending, Presence::Window, window, cx)
    }

    fn hide_secret(&mut self, cx: &mut Context<Self>) {
        self.secret_shown = FormShown::hidden();
        self.editors.show_secret(false, cx);
    }

    fn forget_secret(&mut self, cx: &mut Context<Self>) {
        self.form.forget_value();
        self.hide_secret(cx);
        self.editors.push(&self.form, cx);
    }

    pub fn live_in(
        feed: Arc<SwitchFeed>,
        vault: Arc<Vault>,
        pending: PendingRequest,
        presence: Presence,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let watching = vec![
            lock_watch::observe(cx, |view: &mut Self, cx| {
                if lock_watch::moved(&mut view.seen_lock) {
                    view.hide_secret(cx);
                }
                cx.notify();
            }),
            cx.observe_window_activation(window, |view, window, cx| {
                if !window.is_window_active() {
                    view.hide_secret(cx);
                    cx.notify();
                }
            }),
        ];
        let form = Form::request(&pending);
        let editors = FormEditors::new(
            &form,
            cx,
            Rc::new(|this: &mut Self, e, cx| this.event(e, cx)),
        );
        let view = VaultRequestView {
            feed,
            vault,
            id: pending.id.clone(),
            cover: Cover::new(pending.clone()),
            form,
            editors,
            busy: false,
            flow: Flow::default(),
            presence,
            visible: !presence.embedded(),
            typing: presence.takes_focus(),
            reset_focus: false,
            want_focus: false,
            requested_by: requested_by(pending.verified.as_ref()),
            focus: cx.focus_handle(),
            fixed_today: None,
            copied: None,
            secret_shown: FormShown::hidden(),
            seen_lock: lock_watch::seen(),
            _watching: watching,
        };
        cx.on_release(|this, _| {
            this.lapse(Duration::ZERO);
        })
        .detach();
        cx.on_app_quit(|this, _| {
            this.lapse(Duration::ZERO);
            async {}
        })
        .detach();
        view
    }

    pub fn fixed(pending: PendingRequest, today: i64, cx: &mut Context<Self>) -> Self {
        let form = Form::request(&pending);
        let editors = FormEditors::new(
            &form,
            cx,
            Rc::new(|this: &mut Self, e, cx| this.event(e, cx)),
        );
        VaultRequestView {
            feed: Arc::new(SwitchFeed::fixed(crate::switches::Switches::all_on())),
            vault: Arc::new(Vault::fixed(Vec::new())),
            id: pending.id.clone(),
            cover: Cover::new(pending.clone()),
            form,
            editors,
            busy: false,
            flow: Flow::default(),
            presence: Presence::Window,
            visible: true,
            typing: true,
            reset_focus: false,
            want_focus: false,
            requested_by: requested_by(pending.verified.as_ref()),
            focus: cx.focus_handle(),
            fixed_today: Some(today),
            copied: None,
            secret_shown: FormShown::hidden(),
            seen_lock: lock_watch::seen(),
            _watching: Vec::new(),
        }
    }

    pub fn refused(mut self, refusal: super::consent_save::Refusal) -> Self {
        self.flow.settle(Err(Failure::Refused(refusal)));
        self.form.error = self.flow.line();
        self.cover.settle(Stage::after(&self.flow), &self.feed);
        self
    }

    pub fn with_presence(mut self, presence: Presence) -> Self {
        self.presence = presence;
        self
    }

    pub fn set_presence(&mut self, presence: Presence, cx: &mut Context<Self>) {
        if self.presence == presence {
            return;
        }
        self.presence = presence;
        self.typing = presence.takes_focus();
        self.reset_focus = true;
        cx.notify();
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if !visible {
            self.hide_secret(cx);
            self.typing = self.presence.takes_focus();
            self.reset_focus = true;
        }
        cx.notify();
    }

    pub fn done(&self) -> bool {
        self.cover.stage() == Stage::Done
    }

    pub fn cover(&self) -> &Cover {
        &self.cover
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

    fn save(&mut self, cx: &mut Context<Self>) {
        if !self.form.can_save() || self.busy || !self.vault.is_live() || !self.flow.can_save() {
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
        if !self.flow.begin() {
            return;
        }
        self.busy = true;
        let ends = self.cover.begin(&self.feed);
        self.form.error = None;
        let consent = self.id.clone();
        self.forget_secret(cx);
        let (vault, switches) = (self.vault.clone(), self.feed.switches());
        let (left, feed_left) = (vault.clone(), self.feed.clone());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    vault.save_for_consent(
                        &switches,
                        &entry,
                        value.as_ref(),
                        replacing.as_deref(),
                        &consent,
                    )
                })
                .await;
            let outcome = result.clone();
            if this.update(cx, |view, cx| view.saved(result, cx)).is_err() {
                closing::end_unsaved(&left, &feed_left, ends, &outcome, Duration::ZERO);
            }
        })
        .detach();
        cx.notify();
    }

    fn saved(&mut self, result: Result<(), Failure>, cx: &mut Context<Self>) {
        self.busy = false;
        self.hide_secret(cx);
        self.flow.settle(result);
        self.form.error = self.flow.line();
        self.cover.settle(Stage::after(&self.flow), &self.feed);
        if self.done() {
            self.feed.resolve(&self.id);
            cx.emit(Done);
        }
        cx.notify();
    }

    fn leave(&mut self, wait: Duration) -> bool {
        closing::cancel_form(&mut self.cover, self.busy, &self.feed, &self.vault, wait)
    }

    fn lapse(&mut self, wait: Duration) -> bool {
        closing::lapse(&mut self.cover, self.busy, &self.feed, &self.vault, wait)
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.leave(Duration::ZERO) {
            return;
        }
        self.forget_secret(cx);
        cx.emit(Done);
        cx.notify();
    }

    fn event(&mut self, event: FormEvent, cx: &mut Context<Self>) {
        let today = self.today();
        match event {
            FormEvent::Save => return self.save(cx),
            FormEvent::Cancel => return self.cancel(cx),
            FormEvent::Focus(field) => {
                self.form.focus = field;
                self.typing = true;
                self.want_focus = true;
                self.editors.want(field);
            }
            FormEvent::Edited(field) => {
                if !self.editors.pull(&mut self.form, field, cx) {
                    return;
                }
                self.form.focus = field;
            }
            FormEvent::Enter(field) => {
                match self.form.enter(field) {
                    Enter::Newline => self.editors.insert_newline(field, cx),
                    Enter::Save => self.save(cx),
                    Enter::Ignore => {}
                }
                return;
            }
            FormEvent::Kind(kind) => self.form.set_kind(kind),
            FormEvent::ToggleAsk => self.form.ask = !self.form.ask,
            FormEvent::ToggleAdvanced => self.form.advanced = !self.form.advanced,
            FormEvent::EditHowTo => self.form.editing_how_to = true,
            FormEvent::NoExpiry(none) => self.form.choose_expiry(none, today),
            FormEvent::ConfirmHost => self.form.confirm_host(),
            FormEvent::CopyName => {
                if let Some(name) = copyable(&self.form.name) {
                    copy_name::copy(&mut self.copied, name, cx);
                }
                return;
            }
            FormEvent::ToggleSecret => {
                self.secret_shown = self.secret_shown.toggle(copy_name::now_ms());
                if self.secret_shown.shows(copy_name::now_ms()) {
                    notify_when_hidden(cx);
                }
            }
        }
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, window: &Window, cx: &mut Context<Self>) {
        let editing = self.editors.focused(window, cx);
        if !self.typing && editing.is_none() {
            return;
        }
        let k = &event.keystroke;
        match escape_act(parts_on(cx), self.presence.embedded(), &k.key) {
            EscapeAct::Cancel => {
                self.cancel(cx);
                cx.stop_propagation();
                return;
            }
            EscapeAct::HideParts => {
                set_parts(cx, false);
                cx.stop_propagation();
                return;
            }
            EscapeAct::Pass => {}
        }
        if k.key == "tab" && !k.modifiers.platform && !k.modifiers.control && !k.modifiers.alt {
            if let Some(field) = editing {
                self.form.focus = field;
            }
            if k.modifiers.shift {
                self.form.focus_previous();
            } else {
                self.form.focus_next();
            }
            self.editors.want(self.form.focus);
            cx.stop_propagation();
            cx.notify();
        }
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
        if self.reset_focus {
            self.reset_focus = false;
            if self.focus.contains_focused(window, cx) {
                window.blur();
            }
        }
        let asked = std::mem::take(&mut self.want_focus) && self.visible;
        if (asked || self.presence.takes_focus() && self.visible)
            && !self.focus.contains_focused(window, cx)
            && self.fixed_today.is_none()
        {
            self.editors.want(self.form.focus);
        }
        self.editors.apply_want(window, cx);
        self.editors.push(&self.form, cx);
        self.form.keyboard = self.fixed_today.is_some();
        if self.done() {
            return match self.presence {
                Presence::Window => div().size_full().fill(Ink::Window, a).into_any_element(),
                _ => div().into_any_element(),
            };
        }
        let on: Handler<Self, FormEvent> = Rc::new(|this, event, cx| this.event(event, cx));
        let off = self.feed.switches().message(Switch::Vault);
        let footer = form_footer(&self.form, self.busy, off, a, cx, on.clone());
        let frame = div()
            .id("vault-request")
            .track_focus(&self.focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if this.presence.takes_focus() && !this.focus.contains_focused(window, cx) {
                        window.focus(&this.focus);
                    }
                }),
            )
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| this.key(event, window, cx)),
            );
        if lock_watch::moved(&mut self.seen_lock) || !self.feed.switches().is_on(Switch::Vault) {
            self.hide_secret(cx);
        }
        self.secret_shown = self.secret_shown.sweep(copy_name::now_ms());
        let secret_shown = self.secret_shown.shows(copy_name::now_ms());
        self.editors.show_secret(secret_shown, cx);
        let editing = Editing {
            editors: &self.editors,
            window,
            copied_name: copy_name::shows(&self.copied, self.form.name.trim()),
            secret_shown,
        };
        let body = form_body(&self.form, editing, self.today(), a, cx, on);
        match self.presence {
            Presence::Window => frame
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
                        .child(label(self.form.title(), TypeRole::Heading, Ink::Text, a))
                        .child(div().flex_1())
                        .child(close_button(
                            "sheet-close",
                            a,
                            cx,
                            |this: &mut Self, _, cx| this.cancel(cx),
                        )),
                )
                .children(self.requested_by.as_deref().map(|r| {
                    div()
                        .flex_shrink_0()
                        .px(px(SIDE_PADDING))
                        .pt(px(8.0))
                        .child(label(r, TypeRole::Meta, Ink::TextMuted, a))
                }))
                .child(scrollable(body))
                .child(footer)
                .into_any_element(),
            Presence::Card => frame
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_shrink_0()
                        .px(px(SIDE_PADDING))
                        .py(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .border_b(px(1.0))
                        .border_color(hsla(Ink::Hairline.color(a)))
                        .child(icon(
                            Icon::Permission,
                            TypeRole::Heading.spec().size,
                            Ink::AccentText,
                            a,
                        ))
                        .child(label(self.form.title(), TypeRole::Heading, Ink::Text, a))
                        .child(div().flex_1())
                        .child(close_button(
                            "sheet-close",
                            a,
                            cx,
                            |this: &mut Self, _, cx| this.cancel(cx),
                        )),
                )
                .children(self.requested_by.as_deref().map(|r| {
                    div()
                        .flex_shrink_0()
                        .px(px(SIDE_PADDING))
                        .pt(px(8.0))
                        .child(label(r, TypeRole::Meta, Ink::TextMuted, a))
                }))
                .child(scrollable(body.flex_none().max_h(px(CARD_BODY_MAX))))
                .child(footer)
                .into_any_element(),
            Presence::Sheet => frame
                .size_full()
                .flex()
                .flex_col()
                .child(sheet_header_closing(
                    Icon::Permission,
                    self.form.title(),
                    Vec::new(),
                    a,
                    cx,
                    |this: &mut Self, _, cx| this.cancel(cx),
                ))
                .children(self.requested_by.as_deref().map(|r| {
                    div()
                        .flex_shrink_0()
                        .px(px(SIDE_PADDING))
                        .pt(px(8.0))
                        .child(label(r, TypeRole::Meta, Ink::TextMuted, a))
                }))
                .child(scrollable(body))
                .child(footer)
                .into_any_element(),
        }
    }
}
