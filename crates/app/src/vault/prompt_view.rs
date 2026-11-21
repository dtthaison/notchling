use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    div, prelude::*, px, Context, FocusHandle, Focusable, KeyDownEvent, MouseButton, Render, Window,
};
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::ui::{hsla, icon, label, scrollable, window_appearance, KitStyled};
use notchling_kit::{Icon, Ink, TypeRole};

use super::approval::requested_by;
use super::consent_save::{Failure, Flow};
use super::form::{Enter, Form};
use super::form_view::{form_body, form_footer, Editing, FormEditors, FormEvent};
use super::presence::{Done, Presence};
use super::store::Vault;
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
    form: Form,
    editors: FormEditors,
    busy: bool,
    flow: Flow,
    done: bool,
    presence: Presence,
    visible: bool,
    typing: bool,
    reset_focus: bool,
    want_focus: bool,
    requested_by: Option<String>,
    focus: FocusHandle,
    fixed_today: Option<i64>,
}

impl gpui::EventEmitter<Done> for VaultRequestView {}

impl VaultRequestView {
    pub fn live(
        feed: Arc<SwitchFeed>,
        vault: Arc<Vault>,
        pending: PendingRequest,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::live_in(feed, vault, pending, Presence::Window, cx)
    }

    pub fn live_in(
        feed: Arc<SwitchFeed>,
        vault: Arc<Vault>,
        pending: PendingRequest,
        presence: Presence,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = Form::request(&pending);
        let editors = FormEditors::new(
            &form,
            cx,
            Rc::new(|this: &mut Self, e, cx| this.event(e, cx)),
        );
        VaultRequestView {
            feed,
            vault,
            id: pending.id.clone(),
            form,
            editors,
            busy: false,
            flow: Flow::default(),
            done: false,
            presence,
            visible: !presence.embedded(),
            typing: presence.takes_focus(),
            reset_focus: false,
            want_focus: false,
            requested_by: requested_by(pending.verified.as_ref()),
            focus: cx.focus_handle(),
            fixed_today: None,
        }
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
            form,
            editors,
            busy: false,
            flow: Flow::default(),
            done: false,
            presence: Presence::Window,
            visible: true,
            typing: true,
            reset_focus: false,
            want_focus: false,
            requested_by: requested_by(pending.verified.as_ref()),
            focus: cx.focus_handle(),
            fixed_today: Some(today),
        }
    }

    pub fn refused(mut self, refusal: super::consent_save::Refusal) -> Self {
        self.flow.settle(Err(Failure::Refused(refusal)));
        self.form.error = self.flow.line();
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
            self.typing = self.presence.takes_focus();
            self.reset_focus = true;
        }
        cx.notify();
    }

    pub fn done(&self) -> bool {
        self.done
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
        self.form.error = None;
        let consent = self.id.clone();
        self.form.forget_value();
        let (vault, switches) = (self.vault.clone(), self.feed.switches());
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
            this.update(cx, |view, cx| view.saved(result, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn saved(&mut self, result: Result<(), Failure>, cx: &mut Context<Self>) {
        self.busy = false;
        self.flow.settle(result);
        self.form.error = self.flow.line();
        if self.flow.closes() {
            self.done = true;
            self.feed.resolve(&self.id);
            cx.emit(Done);
        }
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.done = true;
        self.feed.dismiss_request(&self.id);
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
        }
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, window: &Window, cx: &mut Context<Self>) {
        let editing = self.editors.focused(window, cx);
        if !self.typing && editing.is_none() {
            return;
        }
        let k = &event.keystroke;
        if k.key == "escape" {
            self.cancel(cx);
            cx.stop_propagation();
            return;
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
        if self.done {
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
        let editing = Editing {
            editors: &self.editors,
            window,
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
