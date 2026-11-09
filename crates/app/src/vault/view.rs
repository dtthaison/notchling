//! The Secret Vault sheet over GPUI (macOS): the list of saved secrets without their values, with
//! Add, Renew…, Edit and Remove, and the form in place of the list. Every call to the daemon runs
//! on the background executor through the store; while the `vault` switch is off every control is
//! drawn disabled with the daemon's off message. A render has fixed entries and never calls
//! anything.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, AnyElement, AnyView, Context, Div, EventEmitter, FocusHandle, Focusable,
    KeyDownEvent, Render, Stateful, Window,
};
use notchling_engine::vault::entry::VaultEntry;
use notchling_engine::vault::expiry::today_local;
use notchling_engine::vault::url::renew_url;
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, label, scrollable, window_appearance, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::form::{Form, Typed};
use super::form_view::{form_body, form_footer, FormEvent};
use super::list::{list, remove_alert, List, TITLE};
use super::renders::{self, Shown};
use super::store::Vault;
use crate::accounts::flows::Alert;
use crate::accounts::view_rows::{alert_overlay, Handler};
use crate::mac::gate;
use crate::sheets::view::{sheet_header, Dismiss};
use crate::switches::feed::SwitchFeed;
use crate::switches::{Switch, Switches};

/// How often the sheet asks the daemon while it is open.
const REFRESH_EVERY: Duration = Duration::from_secs(5);

pub struct VaultSheetView {
    vault: Arc<Vault>,
    feed: Arc<SwitchFeed>,
    form: Option<Form>,
    /// The entry Remove asks about.
    pub(super) removing: Option<String>,
    busy: bool,
    note: Option<String>,
    focus: FocusHandle,
    /// A render's day and switches; the live ones otherwise.
    fixed: Option<(i64, Switches)>,
    seen_vault: u64,
}

impl EventEmitter<Dismiss> for VaultSheetView {}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl VaultSheetView {
    /// The live sheet: reads the list now, then every few seconds and whenever the feed says the
    /// vault changed.
    pub fn live(vault: Arc<Vault>, feed: Arc<SwitchFeed>, cx: &mut Context<Self>) -> Self {
        let view = VaultSheetView {
            seen_vault: feed.changes().vault,
            vault,
            feed,
            form: None,
            removing: None,
            busy: false,
            note: None,
            focus: cx.focus_handle(),
            fixed: None,
        };
        view.refresh(cx);
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u64;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                ticks += 1;
                let due = this.update(cx, |view, _| {
                    let changes = view.feed.changes().vault;
                    let moved = changes != view.seen_vault;
                    view.seen_vault = changes;
                    moved || ticks.is_multiple_of(REFRESH_EVERY.as_secs())
                });
                match due {
                    Ok(true) => {
                        if this.update(cx, |view, cx| view.refresh(cx)).is_err() {
                            break;
                        }
                    }
                    Ok(false) => {}
                    Err(_) => break,
                }
            }
        })
        .detach();
        view
    }

    /// A render's: the entries as given, `form` open if any, at a fixed day.
    pub fn fixed(
        vault: Arc<Vault>,
        feed: Arc<SwitchFeed>,
        form: Option<Form>,
        today: i64,
        cx: &mut Context<Self>,
    ) -> Self {
        VaultSheetView {
            seen_vault: 0,
            fixed: Some((today, feed.switches())),
            vault,
            feed,
            form,
            removing: None,
            busy: false,
            note: None,
            focus: cx.focus_handle(),
        }
    }

    fn switches(&self) -> Switches {
        match &self.fixed {
            Some((_, s)) => *s,
            None => self.feed.switches(),
        }
    }

    fn today(&self) -> i64 {
        self.fixed
            .map(|(t, _)| t)
            .unwrap_or_else(|| today_local(now_secs()))
    }

    fn refresh(&self, cx: &mut Context<Self>) {
        if !self.vault.is_live() {
            return;
        }
        let (vault, switches) = (self.vault.clone(), self.switches());
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move { vault.refresh(&switches) })
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    /// Runs `job` off the UI thread, then reads the list again. `after` takes the result.
    fn run(
        &mut self,
        job: impl FnOnce(&Vault, &Switches) -> Result<(), String> + Send + 'static,
        after: impl FnOnce(&mut Self, Result<(), String>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.busy || !self.vault.is_live() {
            return;
        }
        self.busy = true;
        self.note = None;
        let (vault, switches) = (self.vault.clone(), self.switches());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let result = job(&vault, &switches);
                    vault.refresh(&switches);
                    result
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                after(view, result, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn open_form(&mut self, form: Form, window: &mut Window, cx: &mut Context<Self>) {
        self.form = Some(form);
        self.note = None;
        window.focus(&self.focus);
        cx.notify();
    }

    pub(super) fn entry(&self, name: &str) -> Option<VaultEntry> {
        self.vault
            .snapshot()
            .entries
            .into_iter()
            .find(|e| e.name == name)
    }

    pub(super) fn renew(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(name) else {
            return;
        };
        if let Some(url) = renew_url(entry.url.as_deref()) {
            cx.open_url(&url);
        }
        self.open_form(Form::renew(&entry), window, cx);
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        if !form.can_save() {
            return;
        }
        let (entry, value, replacing) = match form.to_save() {
            Ok(parts) => parts,
            Err(e) => {
                form.error = Some(e);
                cx.notify();
                return;
            }
        };
        let request = form.request.clone();
        // The value leaves the form the moment it is handed to the save.
        form.forget_value();
        self.run(
            move |v, sw| {
                v.save(
                    sw,
                    &entry,
                    value.as_ref(),
                    replacing.as_deref(),
                    request.as_deref(),
                )
            },
            |view, result, _| match result {
                Ok(()) => view.form = None,
                Err(e) => {
                    if let Some(form) = view.form.as_mut() {
                        form.error = Some(e);
                    }
                }
            },
            cx,
        );
    }

    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if k.key == "escape" {
            if self.form.take().is_some() {
                cx.notify();
            } else {
                cx.emit(Dismiss);
            }
            cx.stop_propagation();
            return;
        }
        let Some(form) = self.form.as_mut() else {
            return;
        };
        if super::form_view::clipboard_key(form, k, cx) {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let modified = k.modifiers.platform || k.modifiers.control;
        match form.key(&k.key, k.key_char.as_deref(), modified) {
            Typed::Submit => self.save(cx),
            Typed::Changed => {}
            Typed::Ignored => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn list_body(&self, l: &List, a: Appearance, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut body = div()
            .id("vault-list")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p(px(SIDE_PADDING))
            .flex()
            .flex_col()
            .gap(px(8.0));
        if let Some(off) = l.off {
            body = body.child(label(off, TypeRole::Meta, Ink::TextMuted, a));
        }
        if let Some((empty, add)) = &l.empty {
            let b = button(
                "add-secret",
                "Add Secret",
                ButtonKind::Primary,
                gate::state(*add),
                a,
            );
            let b = gate::tip(b, *add, a).when(add.enabled, |b| {
                b.on_click(
                    cx.listener(|this, _, window, cx| this.open_form(Form::add(), window, cx)),
                )
            });
            body = body
                .child(label(empty, TypeRole::Body, Ink::Text, a).pt(px(20.0)))
                .child(label(l.usage, TypeRole::Meta, Ink::TextMuted, a))
                .child(div().flex().child(b));
        }
        for row in &l.rows {
            body = body.child(super::view_list::list_row(row, a, cx));
        }
        if !l.rows.is_empty() {
            body = body.child(label(l.usage, TypeRole::Meta, Ink::TextFaint, a).pt(px(6.0)));
        }
        if let Some(e) = l.error.as_deref().or(self.note.as_deref()) {
            body = body.child(label(e, TypeRole::Meta, Ink::Needs, a));
        }
        body
    }
}

impl Focusable for VaultSheetView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for VaultSheetView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let switches = self.switches();
        let snapshot = self.vault.snapshot();
        let today = self.today();
        let (title, body, footer): (&str, AnyElement, Option<Div>) = match &self.form {
            Some(form) => {
                let on: Handler<Self, FormEvent> =
                    Rc::new(|this, event, cx| this.pending_form_event(event, cx));
                let footer_on = on.clone();
                (
                    form.title(),
                    scrollable(form_body(form, today, a, cx, on)).into_any_element(),
                    Some(form_footer(
                        form,
                        self.busy,
                        switches.message(Switch::Vault),
                        a,
                        cx,
                        footer_on,
                    )),
                )
            }
            None => {
                let l = list(&snapshot.entries, &switches, today, snapshot.error.clone());
                (
                    TITLE,
                    scrollable(self.list_body(&l, a, cx)).into_any_element(),
                    None,
                )
            }
        };
        let l = list(&snapshot.entries, &switches, today, None);
        let mut end = div().flex().items_center().gap(px(8.0));
        if self.form.is_none() {
            if let Some(add) = l.add {
                let b = button(
                    "vault-add",
                    "Add",
                    ButtonKind::Secondary,
                    gate::state(add),
                    a,
                );
                end = end.child(gate::tip(b, add, a).when(add.enabled, |b| {
                    b.on_click(
                        cx.listener(|this, _, window, cx| this.open_form(Form::add(), window, cx)),
                    )
                }));
            }
            end = end.child(
                button(
                    "vault-done",
                    "Done",
                    ButtonKind::Secondary,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(|_, _, _, cx| cx.emit(Dismiss))),
            );
        }
        let alert = self.removing.as_ref().map(|name| {
            let (title, message) = remove_alert(name);
            let alert = Alert {
                title,
                message: message.to_owned(),
                button: "Remove".into(),
                destructive: true,
            };
            let on: Handler<Self, bool> = Rc::new(|this, yes, cx| {
                let Some(name) = this.removing.take() else {
                    return;
                };
                if yes {
                    this.run(
                        move |v, sw| v.remove(sw, &name),
                        |view, r, _| view.note = r.err(),
                        cx,
                    );
                }
                cx.notify();
            });
            alert_overlay(&alert, self.busy, a, cx, on)
        });
        div()
            .id("vault-sheet")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key(event, cx)))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .overflow_hidden()
            .fill(Ink::Window, a)
            .child(sheet_header(
                Icon::Permission,
                title,
                end.id("vault-header-end"),
                a,
            ))
            .child(body)
            .children(footer)
            .children(alert)
    }
}

impl VaultSheetView {
    /// A form event from a click: it needs the window for focus, which a render-time handler
    /// doesn't carry, so the focus is taken on the next key instead.
    fn pending_form_event(&mut self, event: FormEvent, cx: &mut Context<Self>) {
        match event {
            FormEvent::Save => self.save(cx),
            FormEvent::Cancel => {
                self.form = None;
                cx.notify();
            }
            FormEvent::Focus(field) => {
                if let Some(form) = self.form.as_mut() {
                    form.focus = field;
                }
                cx.notify();
            }
            other => {
                let today = self.today();
                if let Some(form) = self.form.as_mut() {
                    match other {
                        FormEvent::Kind(kind) => form.set_kind(kind),
                        FormEvent::ToggleAsk => form.ask = !form.ask,
                        FormEvent::ToggleAdvanced => form.advanced = !form.advanced,
                        FormEvent::EditHowTo => form.editing_how_to = true,
                        FormEvent::NoExpiry(none) => form.choose_expiry(none, today),
                        FormEvent::ConfirmHost => form.confirm_host(),
                        _ => {}
                    }
                }
                cx.notify();
            }
        }
    }
}

/// `--render vault-sheet|vault-form|vault-approval|vault-request`, over made-up entries; nothing
/// is called.
pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    use super::approval_view::VaultApprovalView;
    use super::prompt_view::VaultRequestView;
    let which = renders::shown(name)?;
    let vault = Arc::new(Vault::fixed(renders::entries()));
    let feed = Arc::new(SwitchFeed::fixed(Switches::all_on()));
    Some(match which {
        Shown::List => cx
            .new(|cx| VaultSheetView::fixed(vault, feed, None, renders::TODAY, cx))
            .into(),
        Shown::Form => {
            let form = Form::edit(&renders::entries()[0]);
            cx.new(|cx| VaultSheetView::fixed(vault, feed, Some(form), renders::TODAY, cx))
                .into()
        }
        Shown::Approval => {
            let approval = super::approval::pending(&renders::approvals())
                .into_iter()
                .next()?;
            cx.new(|cx| VaultApprovalView::fixed(&approval, cx)).into()
        }
        Shown::Request => cx
            .new(|cx| VaultRequestView::fixed(renders::pending_request(), renders::TODAY, cx))
            .into(),
    })
}
