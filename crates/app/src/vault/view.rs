use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, AnyElement, Context, Div, EventEmitter, FocusHandle, Focusable,
    KeyDownEvent, Render, Stateful, Subscription, Window,
};
use notchling_engine::vault::entry::VaultEntry;
use notchling_engine::vault::expiry::today_local;
use notchling_engine::vault::url::renew_url;
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::part::ButtonKind;
use notchling_kit::ui::{button, label, scrollable, window_appearance, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::copy_name::{self, Copied};
use super::form::{copyable, Enter, Form};
use super::form_view::{form_body, form_footer, Editing, FormEditors, FormEvent};
use super::list::{list, remove_alert, List, TITLE};
use super::lock_watch;
use super::reads::Reads;
use super::reveal::{FormShown, Revealed, Seen};
use super::reveal_view::notify_when_hidden;
use super::store::Vault;
use super::view_list::notice;
use crate::accounts::flows::Alert;
use crate::accounts::view_rows::{alert_overlay, Handler};
use crate::cluster::feed::ClusterFeed;
use crate::cluster::model::VaultNode;
use crate::cluster::secrets::{store_for, target, Target};
use crate::cluster::secrets_view::vault_tabs;
use crate::mac::gate;
use crate::sheets::header::sheet_header_closing;
use crate::sheets::view::Dismiss;
use crate::switches::feed::SwitchFeed;
use crate::switches::{Switch, Switches};

const REFRESH_EVERY: Duration = Duration::from_secs(5);

pub struct VaultSheetView {
    vault: Arc<Vault>,
    feed: Arc<SwitchFeed>,
    cluster: Arc<ClusterFeed>,
    pub(super) remote_tab: bool,
    pin: Option<Target>,
    seen_target: Target,
    pub(super) form: Option<Form>,
    editors: Option<FormEditors>,
    pub(super) removing: Option<String>,
    busy: bool,
    pub(super) note: Option<String>,
    focus: FocusHandle,
    fixed: Option<(i64, Switches)>,
    seen_vault: u64,
    pub(super) copied: Copied,
    pub(super) revealed: Revealed,
    pub(super) usernames: HashMap<String, (String, Option<String>)>,
    pub(super) form_shown: FormShown,
    pub(super) reads: Reads,
    pub(super) seen_lock: Seen,
    _lock: Option<Subscription>,
}

#[derive(Clone)]
pub(super) enum SwitchSource {
    Fixed(Switches),
    Live(Arc<SwitchFeed>),
}

impl SwitchSource {
    pub(super) fn get(&self) -> Switches {
        match self {
            SwitchSource::Fixed(switches) => *switches,
            SwitchSource::Live(feed) => feed.switches(),
        }
    }
}

impl Drop for VaultSheetView {
    fn drop(&mut self) {
        self.hide_everything();
    }
}

impl EventEmitter<Dismiss> for VaultSheetView {}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl VaultSheetView {
    pub fn live(
        vault: Arc<Vault>,
        feed: Arc<SwitchFeed>,
        cluster: Arc<ClusterFeed>,
        cx: &mut Context<Self>,
    ) -> Self {
        let lock = lock_watch::observe(cx, |view: &mut Self, cx| {
            view.sweep_reveals(cx);
            cx.notify();
        });
        let view = VaultSheetView {
            seen_vault: feed.changes().vault,
            vault,
            feed,
            cluster,
            remote_tab: false,
            pin: None,
            seen_target: Target::Local,
            form: None,
            editors: None,
            removing: None,
            busy: false,
            note: None,
            focus: cx.focus_handle(),
            fixed: None,
            copied: None,
            revealed: Revealed::default(),
            usernames: HashMap::new(),
            form_shown: FormShown::hidden(),
            reads: Reads::default(),
            seen_lock: lock_watch::seen(),
            _lock: Some(lock),
        };
        view.refresh(cx);
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u64;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                ticks += 1;
                let due = this.update(cx, |view, cx| {
                    if view.sweep_reveals(cx) {
                        cx.notify();
                    }
                    let changes = view.feed.changes().vault;
                    let target = view.target();
                    let moved = changes != view.seen_vault || target != view.seen_target;
                    view.seen_vault = changes;
                    view.seen_target = target;
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

    pub fn fixed(
        vault: Arc<Vault>,
        feed: Arc<SwitchFeed>,
        cluster: Arc<ClusterFeed>,
        form: Option<Form>,
        today: i64,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = form.map(|mut f| {
            f.keyboard = true;
            f
        });
        let editors = form.as_ref().map(|f| Self::editors_for(f, cx));
        VaultSheetView {
            seen_vault: 0,
            fixed: Some((today, feed.switches())),
            copied: None,
            revealed: Revealed::default(),
            usernames: HashMap::new(),
            form_shown: FormShown::hidden(),
            reads: Reads::default(),
            seen_lock: lock_watch::seen(),
            _lock: None,
            vault,
            feed,
            cluster,
            remote_tab: false,
            pin: None,
            seen_target: Target::Local,
            form,
            editors,
            removing: None,
            busy: false,
            note: None,
            focus: cx.focus_handle(),
        }
    }

    fn editors_for(form: &Form, cx: &mut Context<Self>) -> FormEditors {
        FormEditors::new(
            form,
            cx,
            Rc::new(|this: &mut Self, event, cx| this.pending_form_event(event, cx)),
        )
    }

    pub(super) fn show_form_secret(&self, cx: &mut Context<Self>) {
        if let Some(editors) = self.editors.as_ref() {
            editors.show_secret(self.form_shown.shows(copy_name::now_ms()), cx);
        }
    }

    fn set_form(&mut self, form: Option<Form>, cx: &mut Context<Self>) {
        self.revealed.hide_all();
        self.form_shown = FormShown::hidden();
        self.drop_pending();
        self.editors = form.as_ref().map(|f| Self::editors_for(f, cx));
        self.form = form;
    }

    fn remote(&self) -> Option<VaultNode> {
        self.cluster
            .snapshot()
            .remote_vault()
            .cloned()
            .filter(|_| self.remote_tab)
    }

    fn target(&self) -> Target {
        target(self.remote_tab, &self.cluster.snapshot())
    }

    fn store_pinned(&self, pinned: Option<&Target>) -> Result<Arc<Vault>, String> {
        store_for(
            pinned,
            self.remote_tab,
            &self.cluster.snapshot(),
            &self.vault,
            &self.cluster.remote_vault(),
        )
    }

    pub(super) fn store(&self) -> Option<Arc<Vault>> {
        self.store_pinned(None).ok()
    }

    fn pinned_node(&self) -> Option<String> {
        match &self.pin {
            Some(Target::Remote(id)) => Some(id.clone()),
            _ => None,
        }
    }

    pub(super) fn ask_remove(&mut self, name: &str, cx: &mut Context<Self>) {
        self.pin = Some(self.target());
        self.removing = Some(name.to_owned());
        cx.notify();
    }

    fn refuse(&mut self, error: String) {
        self.removing = None;
        match self.form.as_mut() {
            Some(form) => form.error = Some(error),
            None => self.note = Some(error),
        }
    }

    fn show_tab(&mut self, remote: bool, cx: &mut Context<Self>) {
        if self.remote_tab == remote {
            return;
        }
        self.remote_tab = remote;
        self.hide_everything();
        self.pin = None;
        self.set_form(None, cx);
        self.removing = None;
        self.note = None;
        self.refresh(cx);
        cx.notify();
    }

    pub(super) fn switches(&self) -> Switches {
        if self.remote().is_some() {
            return Switches::all_on();
        }
        match &self.fixed {
            Some((_, s)) => *s,
            None => self.feed.switches(),
        }
    }

    pub(super) fn switch_source(&self) -> SwitchSource {
        if self.remote().is_some() {
            return SwitchSource::Fixed(Switches::all_on());
        }
        match &self.fixed {
            Some((_, switches)) => SwitchSource::Fixed(*switches),
            None => SwitchSource::Live(self.feed.clone()),
        }
    }

    fn today(&self) -> i64 {
        self.fixed
            .map(|(t, _)| t)
            .unwrap_or_else(|| today_local(now_secs()))
    }

    fn refresh(&self, cx: &mut Context<Self>) {
        let Some(vault) = self.store().filter(|v| v.is_live()) else {
            return;
        };
        let switches = self.switches();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move { vault.refresh(&switches) })
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    fn run(
        &mut self,
        job: impl FnOnce(&Vault, &Switches) -> Result<(), String> + Send + 'static,
        after: impl FnOnce(&mut Self, Result<(), String>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        let vault = match self.store_pinned(self.pin.as_ref()) {
            Ok(vault) => vault,
            Err(error) => {
                self.refuse(error);
                cx.notify();
                return;
            }
        };
        if !vault.is_live() {
            return;
        }
        self.busy = true;
        self.note = None;
        let switches = self.switches();
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

    pub fn open_form(&mut self, form: Form, window: &mut Window, cx: &mut Context<Self>) {
        self.pin = Some(self.target());
        let focus = form.focus;
        self.set_form(Some(form), cx);
        if let Some(editors) = self.editors.as_ref() {
            editors.want(focus);
        }
        self.note = None;
        window.focus(&self.focus);
        cx.notify();
    }

    pub fn entry(&self, name: &str) -> Option<VaultEntry> {
        self.store()?
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
        match self.store_pinned(self.pin.as_ref()) {
            Err(error) => {
                self.refuse(error);
                cx.notify();
                return;
            }
            Ok(vault) if self.busy || !vault.is_live() => return,
            Ok(_) => {}
        }
        if let Some(form) = self.form.as_mut() {
            form.forget_value();
        }
        self.form_shown = FormShown::hidden();
        let pinned = self.pinned_node();
        self.run(
            move |v, sw| {
                v.save_at(
                    pinned.as_deref(),
                    sw,
                    &entry,
                    value.as_ref(),
                    (replacing.as_deref(), request.as_deref()),
                )
            },
            |view, result, cx| match result {
                Ok(()) => view.set_form(None, cx),
                Err(e) => {
                    if let Some(form) = view.form.as_mut() {
                        form.error = Some(e);
                    }
                }
            },
            cx,
        );
    }

    fn key(&mut self, event: &KeyDownEvent, window: &Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if k.key == "escape" {
            if self.form.is_some() {
                self.set_form(None, cx);
                cx.notify();
            } else {
                self.hide_everything();
                cx.emit(Dismiss);
            }
            cx.stop_propagation();
            return;
        }
        if k.key == "tab" && !k.modifiers.platform && !k.modifiers.control && !k.modifiers.alt {
            let (Some(form), Some(editors)) = (self.form.as_mut(), self.editors.as_ref()) else {
                return;
            };
            if let Some(field) = editors.focused(window, cx) {
                form.focus = field;
            }
            if k.modifiers.shift {
                form.focus_previous();
            } else {
                form.focus_next();
            }
            editors.want(form.focus);
            cx.stop_propagation();
            cx.notify();
        }
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
            let copied = copy_name::shows(&self.copied, &row.name);
            let lines = self.field_lines(row);
            body = body.child(super::view_list::list_row(row, copied, &lines, a, cx));
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
        let snapshot = self.store().map(|s| s.snapshot()).unwrap_or_default();
        let today = self.today();
        self.sweep_reveals(cx);
        self.guard_reveals(&snapshot.entries, cx);
        let remote = self.remote();
        let unreachable = self.target() == Target::Unreachable;
        let mut tabs = None;
        let known = self
            .cluster
            .snapshot()
            .remote_vault()
            .map(|n| n.name.clone());
        if self.remote_tab || known.is_some() {
            let on: Handler<Self, bool> = Rc::new(|this, remote, cx| this.show_tab(remote, cx));
            tabs = Some(vault_tabs(known.as_deref(), self.remote_tab, a, cx, on));
        }
        let secret_shown = self.form_shown.shows(copy_name::now_ms());
        if let (Some(form), Some(editors)) = (self.form.as_ref(), self.editors.as_ref()) {
            editors.apply_want(window, cx);
            editors.push(form, cx);
            editors.show_secret(secret_shown, cx);
        }
        let (title, body, footer): (&str, AnyElement, Option<Div>) =
            match (&self.form, &self.editors) {
                (Some(form), Some(editors)) => {
                    let on: Handler<Self, FormEvent> =
                        Rc::new(|this, event, cx| this.pending_form_event(event, cx));
                    let footer_on = on.clone();
                    (
                        form.title(),
                        scrollable(form_body(
                            form,
                            Editing {
                                editors,
                                window,
                                copied_name: copy_name::shows(&self.copied, form.name.trim()),
                                secret_shown,
                            },
                            today,
                            a,
                            cx,
                            on,
                        ))
                        .into_any_element(),
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
                (None, _) if unreachable || remote.as_ref().is_some_and(|n| !n.online) => {
                    let text = match &remote {
                        Some(node) => t::cluster::vault_offline(&node.name),
                        None => t::cluster::vault_unreachable().to_owned(),
                    };
                    (TITLE, notice(&text, Ink::Needs, a), None)
                }
                (None, _) if remote.is_some() && !snapshot.attached => (
                    TITLE,
                    notice(t::cluster::secrets_reading(), Ink::TextMuted, a),
                    None,
                ),
                _ => {
                    let l = list(&snapshot.entries, &switches, today, snapshot.error.clone());
                    (
                        TITLE,
                        scrollable(self.list_body(&l, a, cx)).into_any_element(),
                        None,
                    )
                }
            };
        let l = list(&snapshot.entries, &switches, today, None);
        let mut end: Vec<AnyElement> = Vec::new();
        if self.form.is_none() {
            if let Some(add) = l.add {
                let b = button(
                    "vault-add",
                    "Add",
                    ButtonKind::Secondary,
                    gate::state(add),
                    a,
                );
                end.push(
                    gate::tip(b, add, a)
                        .when(add.enabled, |b| {
                            b.on_click(cx.listener(|this, _, window, cx| {
                                this.open_form(Form::add(), window, cx)
                            }))
                        })
                        .into_any_element(),
                );
            }
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
                    let pinned = this.pinned_node();
                    this.run(
                        move |v, sw| v.remove_at(pinned.as_deref(), sw, &name),
                        |view, r, _| view.note = r.err(),
                        cx,
                    );
                }
                cx.notify();
            });
            alert_overlay(&alert, self.busy, a, cx, on)
        });
        let mut sheet = div()
            .id("vault-sheet")
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| this.key(event, window, cx)),
            )
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .overflow_hidden()
            .fill(Ink::Window, a)
            .child(sheet_header_closing(
                Icon::Permission,
                title,
                end,
                a,
                cx,
                |this, _, cx| {
                    if this.form.is_some() {
                        this.pending_form_event(FormEvent::Cancel, cx);
                    } else {
                        this.hide_everything();
                        cx.emit(Dismiss);
                    }
                },
            ))
            .children(tabs.filter(|_| self.form.is_none()))
            .child(body)
            .children(footer)
            .children(alert);
        if self.remote_tab {
            sheet = sheet.part("Remote Vault", a, cx);
        }
        sheet
    }
}

impl VaultSheetView {
    pub(super) fn copy_name(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(name) = copyable(name) {
            copy_name::copy(&mut self.copied, name, cx);
        }
    }

    fn pending_form_event(&mut self, event: FormEvent, cx: &mut Context<Self>) {
        match event {
            FormEvent::Save => self.save(cx),
            FormEvent::Cancel => {
                self.set_form(None, cx);
                cx.notify();
            }
            FormEvent::Focus(field) => {
                if let Some(form) = self.form.as_mut() {
                    form.focus = field;
                }
                if let Some(editors) = self.editors.as_ref() {
                    editors.want(field);
                }
                cx.notify();
            }
            FormEvent::Edited(field) => {
                if let (Some(form), Some(editors)) = (self.form.as_mut(), self.editors.as_ref()) {
                    if editors.pull(form, field, cx) {
                        form.focus = field;
                        cx.notify();
                    }
                }
            }
            FormEvent::Enter(field) => {
                let enter = self.form.as_ref().map(|f| f.enter(field));
                match (enter, self.editors.as_ref()) {
                    (Some(Enter::Newline), Some(editors)) => editors.insert_newline(field, cx),
                    (Some(Enter::Save), _) => self.save(cx),
                    _ => {}
                }
            }
            FormEvent::ToggleSecret => {
                self.form_shown = self.form_shown.toggle(copy_name::now_ms());
                if self.form_shown.shows(copy_name::now_ms()) {
                    notify_when_hidden(cx);
                }
                cx.notify();
            }
            FormEvent::CopyName => {
                let name = self
                    .form
                    .as_ref()
                    .and_then(|f| copyable(&f.name))
                    .map(str::to_owned);
                if let Some(name) = name {
                    copy_name::copy(&mut self.copied, &name, cx);
                }
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

pub use super::view_render::build_render;
