use std::collections::BTreeSet;
use std::time::Instant;

use gpui::{
    div, prelude::*, px, Context, Div, Entity, FocusHandle, KeyDownEvent, Subscription, Window,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{SPACE, SPINNER_SMALL};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, hsla, icon, label, spinner, KitPart, TextEditor, TextEditorEvent,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::add::AddFlow;
use super::add_parts::AddParts;
use super::add_runs::Snap;
use super::add_sheet::{
    derived_name, install, machine_view, may_open, refused_view, sign_in_target, view, AddPassword,
    AddView, Button, Draft,
};
use super::address_rows_view::{RowEditor, Rows};
use super::addresses_form::{RowDraft, RowMeta};
use super::machine_add::{MachineAdd, Probe};
use super::model::ClusterModel;
use super::not_joined::NotJoined;
use super::view::{field_box, ClusterSheetView};
use crate::machines::view::mono_lines;
use crate::notch::answer::{type_key, Typed};
use crate::sheets::view::Dismiss;
use crate::vault::form::Form;
use crate::vault::pick::{local_entries, VaultPick};
use crate::vault::pick_view::PickEvent;
use crate::vault::view::VaultSheetView;

const SHEET_WIDTH: f32 = 560.0;
const SHEET_MARGIN: f32 = 60.0;
const PREVIEW_HEIGHT: f32 = 1100.0;
const NOT_JOINED_EVERY_SECS: u64 = 5;

pub(super) struct AddSheet {
    pub(super) address: Entity<TextEditor>,
    pub(super) name: Entity<TextEditor>,
    pub(super) rows: Vec<RowEditor>,
    pub(super) touched: bool,
    pub(super) machine: MachineAdd,
    pub(super) flow: Option<String>,
    pub(super) joining: bool,
    pub(super) parts: Option<AddParts>,
    pub(super) user: String,
    pub(super) names: Vec<String>,
    pub(super) password: AddPassword,
    pub(super) vault: VaultPick,
    pub(super) focus: FocusHandle,
    pub(super) commands: BTreeSet<String>,
    pub(super) error: Option<String>,
    _address_events: Subscription,
    _name_events: Subscription,
}

pub(super) struct NestedVault {
    entity: Entity<VaultSheetView>,
    _closing: Subscription,
}

#[derive(Default)]
pub(super) struct AddUi {
    pub(super) sheet: Option<AddSheet>,
    nested: Option<NestedVault>,
    pub(super) tall: bool,
    joined: Option<(u64, Instant, Vec<NotJoined>)>,
}

impl AddUi {
    pub(super) fn forget_list(&mut self) {
        self.joined = None;
    }

    fn not_joined(
        &mut self,
        feed: &super::feed::ClusterFeed,
        model: &ClusterModel,
    ) -> Vec<NotJoined> {
        let generation = feed.generation();
        if let Some((seen, at, list)) = &self.joined {
            if *seen == generation && at.elapsed().as_secs() < NOT_JOINED_EVERY_SECS {
                return list.clone();
            }
        }
        let list = feed.add_source().not_joined(model);
        self.joined = Some((generation, Instant::now(), list.clone()));
        list
    }
}

impl AddSheet {
    pub(super) fn new(
        parts: Option<AddParts>,
        flow: Option<String>,
        cx: &mut Context<ClusterSheetView>,
    ) -> AddSheet {
        let address = cx
            .new(|cx| TextEditor::new(t::cluster::add_address_placeholder(), 1, cx).single_line());
        let name = cx.new(|cx| TextEditor::new("", 1, cx).single_line());
        let address_events =
            cx.subscribe(
                &address,
                |this, _, event: &TextEditorEvent, cx| match event {
                    TextEditorEvent::Changed => this.add_address_changed(cx),
                    TextEditorEvent::Submit => this.add_submit(Button::Add, cx),
                },
            );
        let name_events = cx.subscribe(&name, |this, _, event: &TextEditorEvent, cx| match event {
            TextEditorEvent::Changed => this.add_name_changed(cx),
            TextEditorEvent::Submit => this.add_submit(Button::Add, cx),
        });
        let (user, names) = parts
            .as_ref()
            .map(|p| (p.this.user.clone(), p.names()))
            .unwrap_or_default();
        let first = RowEditor {
            editor: address.clone(),
            meta: RowMeta::default(),
            _events: None,
        };
        AddSheet {
            address,
            name,
            rows: vec![first],
            touched: false,
            machine: MachineAdd::new(),
            joining: flow.is_some(),
            flow,
            parts,
            user,
            names,
            password: AddPassword::default(),
            vault: VaultPick::default(),
            focus: cx.focus_handle(),
            commands: BTreeSet::new(),
            error: None,
            _address_events: address_events,
            _name_events: name_events,
        }
    }
}

fn kind_of(button: Button) -> ButtonKind {
    match button {
        Button::Add | Button::Install | Button::Done => ButtonKind::Primary,
        Button::CheckAgain => ButtonKind::Secondary,
        Button::OpenSignIn | Button::FromTailscale => ButtonKind::Secondary,
        Button::Close | Button::Cancel => ButtonKind::Ghost,
    }
}

fn title_of(button: Button) -> &'static str {
    match button {
        Button::Add => t::cluster::add_save(),
        Button::CheckAgain => t::cluster::add_check_again(),
        Button::Install => t::cluster::add_install(),
        Button::OpenSignIn => t::cluster::add_open_sign_in(),
        Button::FromTailscale => t::cluster::tailscale_button(),
        Button::Close => t::cluster::add_close(),
        Button::Cancel => t::cluster::add_cancel(),
        Button::Done => t::cluster::add_finish(),
    }
}

fn id_of(button: Button) -> &'static str {
    match button {
        Button::Add => "add-save",
        Button::CheckAgain => "add-check-again",
        Button::Install => "add-install",
        Button::OpenSignIn => "add-open-sign-in",
        Button::FromTailscale => "add-from-tailscale",
        Button::Close => "add-close",
        Button::Cancel => "add-cancel",
        Button::Done => "add-done",
    }
}

impl ClusterSheetView {
    pub(super) fn open_add(&mut self, cx: &mut Context<Self>) {
        if !may_open(self.busy, self.add.sheet.is_some()) {
            return;
        }
        let parts = self.feed.add_source().parts();
        self.via_names = parts.as_ref().map(|p| p.names()).unwrap_or_default();
        if let Some(parts) = &parts {
            self.load_tailnets(parts, cx);
        }
        self.add.sheet = Some(AddSheet::new(parts, None, cx));
        cx.notify();
    }

    pub(super) fn open_join(&mut self, name: &str, cx: &mut Context<Self>) {
        if !may_open(self.busy, self.add.sheet.is_some()) {
            return;
        }
        let parts = self.feed.add_source().parts();
        let mut sheet = AddSheet::new(parts.clone(), Some(name.to_owned()), cx);
        let runs = self.feed.adds();
        match parts {
            Some(parts) => match AddFlow::for_machine(parts, name) {
                Ok(flow) => {
                    runs.begin(flow);
                }
                Err(why) => {
                    sheet.flow = None;
                    sheet.error = Some(why);
                }
            },
            None => sheet.flow = None,
        }
        self.add.sheet = Some(sheet);
        cx.notify();
    }

    pub(super) fn show_add(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.add.sheet.is_some() {
            return;
        }
        let parts = self.feed.add_source().parts();
        self.add.sheet = Some(AddSheet::new(parts, Some(name.to_owned()), cx));
        cx.notify();
    }

    fn add_address_changed(&mut self, cx: &mut Context<Self>) {
        let Some(sheet) = self.add.sheet.as_mut() else {
            return;
        };
        sheet.error = None;
        if !sheet.touched {
            let typed = sheet.address.read(cx).text().to_owned();
            let derived = derived_name(&typed, &sheet.user);
            sheet
                .name
                .update(cx, |editor, cx| editor.set_text(&derived, cx));
        }
        cx.notify();
    }

    fn add_name_changed(&mut self, cx: &mut Context<Self>) {
        let Some(sheet) = self.add.sheet.as_mut() else {
            return;
        };
        sheet.error = None;
        let typed = sheet.address.read(cx).text().to_owned();
        let name = sheet.name.read(cx).text().to_owned();
        sheet.touched = !name.is_empty() && name != derived_name(&typed, &sheet.user);
        cx.notify();
    }

    fn add_draft(&self, cx: &Context<Self>) -> Draft {
        match &self.add.sheet {
            Some(sheet) => {
                let rows = self.rows_drafts(Rows::Add, cx);
                let mut rest = rows.into_iter();
                let first = rest.next().unwrap_or_default();
                Draft {
                    address: sheet.address.read(cx).text().to_owned(),
                    name: sheet.name.read(cx).text().to_owned(),
                    first: first.meta,
                    more: rest.collect::<Vec<RowDraft>>(),
                }
            }
            None => Draft::default(),
        }
    }

    fn add_shown(&self, cx: &Context<Self>) -> Option<AddView> {
        let sheet = self.add.sheet.as_ref()?;
        let snap = sheet.flow.as_deref().and_then(|n| self.feed.adds().get(n));
        let draft = self.add_draft(cx);
        if snap.is_none() && sheet.joining {
            return Some(refused_view());
        }
        if snap.is_none() {
            return Some(machine_view(
                sheet.machine.phase(),
                &draft,
                &sheet.user,
                &sheet.names,
            ));
        }
        Some(view(
            snap.as_ref(),
            &draft,
            sheet.password.len(),
            &sheet.user,
            &sheet.names,
        ))
    }

    fn add_submit(&mut self, which: Button, cx: &mut Context<Self>) {
        if self.add_shown(cx).is_some_and(|shown| shown.allows(which)) {
            self.add_button(which, cx);
        }
    }

    pub(super) fn add_button(&mut self, which: Button, cx: &mut Context<Self>) {
        let draft = self.add_draft(cx);
        let runs = self.feed.adds();
        let Some(sheet) = self.add.sheet.as_mut() else {
            return;
        };
        match which {
            Button::Add => {
                let Some(parts) = sheet.parts.clone() else {
                    return;
                };
                match sheet.machine.begin(&parts, &draft, None) {
                    Ok(probe) => {
                        sheet.error = None;
                        self.run_machine_probe(probe, parts, cx);
                    }
                    Err(why) => sheet.error = Some(why),
                }
            }
            Button::CheckAgain => {
                if let Some(name) = sheet.flow.clone() {
                    sheet.error = None;
                    runs.check(&name);
                } else if let Some(parts) = sheet.parts.clone() {
                    match sheet.machine.begin(&parts, &draft, None) {
                        Ok(probe) => {
                            sheet.error = None;
                            self.run_machine_probe(probe, parts, cx);
                        }
                        Err(why) => sheet.error = Some(why),
                    }
                }
            }
            Button::Install => {
                let Some(name) = sheet.flow.clone() else {
                    return;
                };
                match install(&mut sheet.password, |password| runs.run(&name, password)) {
                    Ok(()) => sheet.error = None,
                    Err(why) => sheet.error = Some(why),
                }
            }
            Button::OpenSignIn => {
                let target = sheet
                    .flow
                    .as_deref()
                    .and_then(|name| runs.get(name))
                    .as_ref()
                    .and_then(sign_in_target)
                    .map(str::to_owned);
                if let Some(url) = target {
                    cx.open_url(&url);
                }
            }
            Button::FromTailscale => {
                self.add.sheet = None;
                self.add.nested = None;
                self.open_tailscale(cx);
                return;
            }
            Button::Close => {
                self.add.sheet = None;
                self.add.nested = None;
            }
            Button::Cancel => {
                if let Some(name) = sheet.flow.clone() {
                    runs.dismiss(&name);
                }
                self.add.sheet = None;
                self.add.nested = None;
            }
            Button::Done => {
                if let Some(name) = sheet.flow.clone() {
                    runs.dismiss(&name);
                }
                self.add.sheet = None;
                self.add.nested = None;
            }
        }
        cx.notify();
    }

    fn run_machine_probe(&mut self, probe: Probe, parts: AddParts, cx: &mut Context<Self>) {
        let engine = parts.engine.clone();
        cx.spawn(async move |this, cx| {
            let answer = cx
                .background_executor()
                .spawn(async move { probe.run(&engine) })
                .await;
            this.update(cx, |view, cx| {
                let source = view.feed.add_source();
                if let Some(sheet) = view.add.sheet.as_mut() {
                    if sheet.machine.answered(&parts, answer) {
                        if let Some(name) = sheet.machine.name() {
                            source.note_checked(name);
                        }
                        view.add.forget_list();
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn add_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let modified = k.modifiers.platform || k.modifiers.control;
        let paste = k.modifiers.platform && !k.modifiers.control && k.key == "v";
        let pasted = paste
            .then(|| cx.read_from_clipboard().and_then(|c| c.text()))
            .flatten()
            .map(|t| t.chars().filter(|c| !c.is_control()).collect::<String>());
        let typed = {
            let Some(sheet) = self.add.sheet.as_mut() else {
                return;
            };
            match pasted {
                Some(text) => {
                    sheet.password.field().push_str(&text);
                    Typed::Changed
                }
                None => type_key(
                    sheet.password.field(),
                    &k.key,
                    k.key_char.as_deref(),
                    modified,
                ),
            }
        };
        match typed {
            Typed::Ignored => return,
            Typed::Changed => {}
            Typed::Submit => self.add_submit(Button::Install, cx),
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn add_vault_event(
        &mut self,
        event: PickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let runs = self.feed.adds();
        let Some(sheet) = self.add.sheet.as_mut() else {
            return;
        };
        let Some(name) = sheet.flow.clone() else {
            return;
        };
        match event {
            PickEvent::Toggle => {
                if sheet.vault.toggle() {
                    self.read_vault_entries(cx);
                }
            }
            PickEvent::Choose(id) => {
                sheet.error = runs.set_admin_entry(&name, Some(id)).err();
                if sheet.error.is_none() {
                    sheet.password.clear();
                }
                sheet.vault.close();
            }
            PickEvent::Clear => {
                sheet.error = runs.set_admin_entry(&name, None).err();
                sheet.vault.close();
            }
            PickEvent::New => self.open_new_entry(window, cx),
        }
        cx.notify();
    }

    pub(super) fn read_vault_entries(&self, cx: &mut Context<Self>) {
        let (local, switches) = (self.access.local.clone(), self.access.switches.switches());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { local_entries(&local, &switches) })
                .await;
            this.update(cx, |view, cx| {
                view.update.vault_loaded(result.clone());
                if let Some(sheet) = view.add.sheet.as_mut() {
                    sheet.vault.loaded(result);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn open_new_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (vault, switches, cluster) = (
            self.access.local.clone(),
            self.access.switches.clone(),
            self.feed.clone(),
        );
        let entity = cx.new(|cx| VaultSheetView::live(vault, switches, cluster, cx));
        entity.update(cx, |sheet, cx| sheet.open_form(Form::add(), window, cx));
        let closing = cx.subscribe(&entity, |this, _, _: &Dismiss, cx| {
            this.add.nested = None;
            if let Some(sheet) = this.add.sheet.as_mut() {
                sheet.vault.reread();
            }
            this.update.vault_reread();
            this.read_vault_entries(cx);
            cx.notify();
        });
        self.add.nested = Some(NestedVault {
            entity,
            _closing: closing,
        });
    }

    pub(super) fn vault_overlay(&self) -> Option<Div> {
        let nested = self.add.nested.as_ref()?;
        Some(
            div()
                .absolute()
                .size_full()
                .occlude()
                .child(nested.entity.clone()),
        )
    }

    pub(super) fn add_set_master(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(name) = self.add.sheet.as_ref().and_then(|s| s.flow.clone()) {
            self.feed.adds().set_can_be_master(&name, on);
        }
        cx.notify();
    }

    pub(super) fn add_set_acting(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(name) = self.add.sheet.as_ref().and_then(|s| s.flow.clone()) {
            self.feed.adds().set_acting(&name, on);
        }
        cx.notify();
    }

    pub(super) fn add_toggle_commands(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(sheet) = self.add.sheet.as_mut() {
            if !sheet.commands.remove(id) {
                sheet.commands.insert(id.to_owned());
            }
        }
        cx.notify();
    }

    pub(super) fn add_machine_button(
        &self,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let busy = self.busy;
        button(
            "cluster-add-machine",
            t::cluster::add_machine(),
            ButtonKind::Secondary,
            self.state_for_add(),
            a,
        )
        .when(!busy, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.open_add(cx)))
        })
    }

    fn state_for_add(&self) -> PartState {
        if self.busy {
            PartState::Disabled
        } else {
            PartState::Rest
        }
    }

    pub(super) fn not_joined_list(&mut self, model: &ClusterModel) -> Vec<NotJoined> {
        self.add.not_joined(&self.feed, model)
    }

    pub(super) fn add_overlay(
        &self,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let sheet = self.add.sheet.as_ref()?;
        let snap = sheet.flow.as_deref().and_then(|n| self.feed.adds().get(n));
        let shown = self.add_shown(cx)?;
        let mut inner = div().flex().flex_col().gap(px(SPACE[1]));
        if sheet.parts.is_none() {
            inner = inner.child(label(
                t::cluster::add_unavailable(),
                TypeRole::Body,
                Ink::TextMuted,
                a,
            ));
        } else {
            inner = self.add_body(inner, &shown, snap.as_ref(), window, a, cx);
        }
        if let Some(error) = &sheet.error {
            inner = inner.child(label(error, TypeRole::Meta, Ink::Needs, a));
        }
        let body = div()
            .id("add-body")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .child(inner);
        let mut buttons = div()
            .pt(px(SPACE[1]))
            .flex()
            .justify_end()
            .gap(px(SPACE[1]));
        if sheet.parts.is_none() {
            buttons = buttons.child(self.add_action(Button::Close, true, a, cx));
        } else {
            for action in &shown.actions {
                buttons = buttons.child(self.add_action(action.button, action.enabled, a, cx));
            }
        }
        Some(
            div()
                .absolute()
                .size_full()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(Ink::Shadow.color(a)))
                .child(
                    card(a)
                        .w(px(SHEET_WIDTH))
                        .max_h(px(if self.add.tall {
                            PREVIEW_HEIGHT
                        } else {
                            super::HEIGHT - SHEET_MARGIN
                        }))
                        .p(px(SPACE[3]))
                        .flex()
                        .flex_col()
                        .gap(px(SPACE[1]))
                        .child(label(&shown.title, TypeRole::Heading, Ink::Text, a))
                        .child(body)
                        .child(buttons)
                        .part_at("Add Machine Sheet", Corner::TopTrailing, a, cx),
                ),
        )
    }

    fn add_action(
        &self,
        which: Button,
        enabled: bool,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = if enabled {
            PartState::Rest
        } else {
            PartState::Disabled
        };
        let element = button(id_of(which), title_of(which), kind_of(which), state, a)
            .when(enabled, |b| {
                b.on_click(cx.listener(move |this, _, _, cx| this.add_button(which, cx)))
            });
        div().child(element)
    }

    fn add_body(
        &self,
        mut body: Div,
        shown: &AddView,
        snap: Option<&Snap>,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(sheet) = self.add.sheet.as_ref() else {
            return body;
        };
        if shown.form {
            let name_focused = sheet.name.read(cx).is_focused(window);
            body = body
                .child(label(
                    t::cluster::add_intro(),
                    TypeRole::Body,
                    Ink::TextMuted,
                    a,
                ))
                .child(self.rows_view(Rows::Add, window, a, cx))
                .children(
                    shown
                        .target
                        .as_deref()
                        .map(|text| label(text, TypeRole::Meta, Ink::TextMuted, a)),
                )
                .child(label(
                    t::cluster::add_name_label(),
                    TypeRole::Label,
                    Ink::TextFaint,
                    a,
                ))
                .child(field_box(&sheet.name, name_focused, a));
            if let Some(problem) = &shown.error {
                body = body.child(label(problem, TypeRole::Meta, Ink::Needs, a));
            }
        }
        if let Some(text) = &shown.busy {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(SPACE[1]))
                    .child(spinner("add-busy", SPINNER_SMALL, Ink::TextMuted, a))
                    .child(label(text, TypeRole::Body, Ink::TextMuted, a)),
            );
        }
        if let Some(text) = &shown.plan_intro {
            body = body.child(label(text, TypeRole::Body, Ink::TextMuted, a));
        }
        if let Some(plan) = &shown.plan {
            body = body.child(self.plan_block(plan, snap, window, a, cx));
        }
        if let Some(text) = &shown.step {
            body = body.child(label(text, TypeRole::Heading, Ink::Text, a));
        }
        if let Some(text) = &shown.step_title {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(SPACE[1]))
                    .child(spinner("add-step", SPINNER_SMALL, Ink::Working, a))
                    .child(label(text, TypeRole::Body, Ink::Text, a)),
            );
        }
        if let Some(text) = &shown.done {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(SPACE[1]))
                    .child(icon(Icon::Check, TypeRole::Body.spec().size, Ink::Done, a))
                    .child(label(text, TypeRole::Body, Ink::Text, a)),
            );
        }
        if let Some(stop) = &shown.stop {
            body = body.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(SPACE[0]))
                    .child(icon(
                        Icon::Warning,
                        TypeRole::Body.spec().size,
                        Ink::Needs,
                        a,
                    ))
                    .child(div().flex_1().min_w(px(0.0)).child(label(
                        &stop.reason,
                        TypeRole::Body,
                        Ink::Needs,
                        a,
                    ))),
            );
            if let Some(howto) = &stop.howto {
                body = body.child(mono_lines("add-howto", howto, 160.0, a));
            }
        }
        if !shown.log.is_empty() {
            body = body
                .child(label(
                    t::cluster::add_log_label(),
                    TypeRole::Label,
                    Ink::TextFaint,
                    a,
                ))
                .child(mono_lines("add-log", &shown.log.join("\n"), 180.0, a));
        }
        if let Some(text) = &shown.note {
            body = body.child(label(text, TypeRole::Meta, Ink::TextMuted, a));
        }
        body
    }
}
