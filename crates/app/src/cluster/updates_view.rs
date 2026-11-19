use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Instant;

use gpui::{div, prelude::*, px, Context, Div, FocusHandle, KeyDownEvent, SharedString, Window};
use notchling_engine::machines::node_updates::UpdateRow;
use notchling_engine::vault::entry::VaultEntry;
use notchling_kit::labels::Corner;
use notchling_kit::metric::{SPACE, SPINNER_SMALL};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, field, hsla, icon, label, spinner, KitPart};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::add_sheet::AddPassword;
use super::model::ClusterModel;
use super::updates::{banner, commands, execute, idle_lines, lines, run_label, selected, Kind};
use super::updates_admin::{
    apply, blocked, prepare, source, Ask, Need, Source, VaultSaver, VaultState,
};
use super::updates_state::{Offered, UpdateSession};
use super::view::ClusterSheetView;
use crate::machines::sheets_view::checkbox;
use crate::machines::view::mono_lines;
use crate::notch::answer::{type_key, Typed};
use crate::sheets::header::{card_title, close_button};
use crate::vault::pick::VaultPick;
use crate::vault::pick_view::{pick_list, PickEvent};

const SHEET_WIDTH: f32 = 560.0;
const SHEET_MAX_HEIGHT: f32 = super::HEIGHT - 60.0;
const EVERY_SECS: u64 = 5;

pub(super) struct UpdateUi {
    session: Arc<UpdateSession>,
    pub(super) open: bool,
    anyway: BTreeSet<String>,
    deselected: BTreeSet<String>,
    offered: Offered,
    commands: bool,
    passwords: BTreeMap<String, AddPassword>,
    focus: BTreeMap<String, FocusHandle>,
    vault: VaultPick,
    picking: Option<String>,
    entries: VaultState,
    needs: Vec<Need>,
    notice: Option<String>,
    seen: Option<(u64, Instant, Option<String>, Vec<UpdateRow>)>,
}

impl UpdateUi {
    pub(super) fn vault_loaded(&mut self, result: Result<Vec<VaultEntry>, String>) {
        self.entries = VaultState::from_result(&result);
        self.vault.loaded(result);
    }

    pub(super) fn vault_reread(&mut self) {
        self.vault.reread();
    }

    pub(super) fn new(session: Arc<UpdateSession>) -> UpdateUi {
        UpdateUi {
            session,
            open: false,
            anyway: BTreeSet::new(),
            deselected: BTreeSet::new(),
            offered: Offered::default(),
            commands: false,
            passwords: BTreeMap::new(),
            focus: BTreeMap::new(),
            vault: VaultPick::default(),
            picking: None,
            entries: VaultState::Reading,
            needs: Vec::new(),
            notice: None,
            seen: None,
        }
    }

    fn offer(
        &mut self,
        feed: &super::feed::ClusterFeed,
        model: &ClusterModel,
    ) -> (Option<String>, Vec<UpdateRow>) {
        let generation = feed.generation();
        if let Some((seen, at, build, rows)) = &self.seen {
            if *seen == generation && at.elapsed().as_secs() < EVERY_SECS {
                return (build.clone(), rows.clone());
            }
        }
        let (build, rows) = feed.add_source().updates(model);
        self.seen = Some((generation, Instant::now(), build.clone(), rows.clone()));
        (build, rows)
    }

    fn sources(&self) -> Vec<(Need, Source)> {
        self.needs
            .iter()
            .map(|need| (need.clone(), source(need, &self.entries)))
            .collect()
    }

    fn typed(&self, machine: &str) -> bool {
        self.passwords
            .get(machine)
            .is_some_and(|password| !password.is_empty())
    }

    fn forget_passwords(&mut self) {
        self.passwords.clear();
        self.picking = None;
        self.vault.close();
    }
}

impl ClusterSheetView {
    pub(super) fn update_banner_row(
        &mut self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let (build, rows) = self.update.offer(&self.feed, model);
        if self.update.session.hides(build.as_deref()) && !self.update.open {
            return None;
        }
        let shown = banner(&rows)?;
        let open = button(
            "cluster-update-open",
            &shown.button,
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| this.open_update(cx)));
        let later = button(
            "cluster-update-not-now",
            t::cluster::update_not_now(),
            ButtonKind::Ghost,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            let build = this.update.seen.as_ref().and_then(|s| s.2.clone());
            this.update.session.hide(build.as_deref());
            cx.notify();
        }));
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(icon(
                    Icon::Info,
                    TypeRole::Meta.spec().size,
                    Ink::AccentText,
                    a,
                ))
                .child(div().flex_1().min_w(px(0.0)).child(label(
                    &shown.text,
                    TypeRole::Meta,
                    Ink::AccentText,
                    a,
                )))
                .child(open)
                .child(later)
                .part("Update Machines", a, cx),
        )
    }

    pub(super) fn preview_update(&mut self, cx: &mut Context<Self>) {
        self.open_update(cx);
        self.update.commands = false;
        let mut items = vec!["github", "npm", "registry"];
        let state = super::add::renders::update_state();
        if matches!(state.as_str(), "linked" | "two" | "running") {
            items.push("m1-admin");
        }
        self.update.entries = VaultState::Items(items.into_iter().map(str::to_owned).collect());
        if state.is_empty() || state == "nolink" {
            self.update
                .passwords
                .entry("m1".to_owned())
                .or_default()
                .field()
                .push_str("hunter22");
        }
        if state == "running" {
            self.update.session.begin();
        }
    }

    pub(super) fn open_update(&mut self, cx: &mut Context<Self>) {
        if self.update.open {
            return;
        }
        self.update.open = true;
        self.update.anyway.clear();
        self.update.deselected.clear();
        self.update.commands = false;
        self.update.forget_passwords();
        self.update.notice = None;
        self.update.entries = VaultState::Reading;
        self.update.needs.clear();
        self.update.seen = None;
        let parts = self.feed.add_source().parts();
        self.update
            .offered
            .reset(parts.as_ref().map(|p| &*p.engine));
        let feed = self.feed.clone();
        if feed.is_live() {
            self.read_vault_entries(cx);
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .spawn(async move { feed.refresh() })
                    .await;
                this.update(cx, |view, cx| {
                    view.update.seen = None;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    fn rows(&mut self) -> Vec<UpdateRow> {
        let model = self.feed.snapshot();
        self.update.offer(&self.feed, &model).1
    }

    fn sync_plan(&mut self, rows: &[UpdateRow]) {
        if self.update.session.running() {
            return;
        }
        let Some(parts) = self.feed.add_source().parts() else {
            return;
        };
        let names = selected(rows, &self.update.anyway, &self.update.deselected);
        if self.update.offered.sync_names(&parts.engine, names) {
            self.refresh_needs();
        }
    }

    fn refresh_needs(&mut self) {
        let machines = self
            .feed
            .add_source()
            .parts()
            .map(|p| p.registry.all())
            .unwrap_or_default();
        self.update.needs =
            super::updates_admin::needs(&machines, &self.update.offered.admin_machines());
    }

    fn close_update(&mut self, cx: &mut Context<Self>) {
        self.update.session.dismissed();
        if !self.update.session.running() {
            let parts = self.feed.add_source().parts();
            self.update
                .offered
                .reset(parts.as_ref().map(|p| &*p.engine));
        }
        self.update.forget_passwords();
        self.update.notice = None;
        self.update.open = false;
        cx.notify();
    }

    fn update_anyway(&mut self, machine: &str, cx: &mut Context<Self>) {
        if self.update.session.running() {
            return;
        }
        self.update.anyway.insert(machine.to_owned());
        self.update.deselected.remove(machine);
        cx.notify();
    }

    fn update_toggle(&mut self, machine: &str, cx: &mut Context<Self>) {
        if self.update.session.running() {
            return;
        }
        if !self.update.deselected.remove(machine) {
            self.update.deselected.insert(machine.to_owned());
        }
        cx.notify();
    }

    fn update_blocked(&self) -> Option<super::updates_admin::Blocked> {
        blocked(
            &self.update.sources(),
            &|machine| self.update.typed(machine),
            &self.update.entries,
        )
    }

    fn start_update(&mut self, cx: &mut Context<Self>) {
        let Some(parts) = self.feed.add_source().parts() else {
            return;
        };
        if self.update.session.running()
            || self.update.offered.shown().is_none()
            || self.update_blocked().is_some()
        {
            return;
        }
        let sources = self.update.sources();
        let mut typed = BTreeMap::new();
        for (need, source) in &sources {
            if matches!(source, Source::Ask(_)) {
                if let Some(secret) = self
                    .update
                    .passwords
                    .get_mut(&need.machine)
                    .and_then(AddPassword::take)
                {
                    typed.insert(need.machine.clone(), secret);
                }
            }
        }
        let prepared = prepare(&sources, typed);
        let Some(plan) = self.update.offered.take() else {
            return;
        };
        if !self.update.session.begin() {
            parts.engine.cancel(plan.id);
            return;
        }
        self.update.forget_passwords();
        self.update.notice = None;
        let session = self.update.session.clone();
        let engine = parts.engine.clone();
        let feed = self.feed.clone();
        let vault = self.access.local.clone();
        let switches = self.access.switches.switches();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move {
                    let saver = VaultSaver {
                        vault: &vault,
                        switches: &switches,
                    };
                    let outcome = match apply(prepared, &*engine, &saver) {
                        Ok(typed) => execute(&engine, &plan, &typed),
                        Err(why) => {
                            engine.cancel(plan.id);
                            notchling_engine::machines::step::FixOutcome::fail(why)
                        }
                    };
                    feed.refresh();
                    session.finish(outcome);
                })
                .await;
            this.update(cx, |view, cx| {
                view.update.seen = None;
                view.update.forget_passwords();
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn update_pick(&mut self, machine: &str, cx: &mut Context<Self>) {
        let same = self.update.picking.as_deref() == Some(machine);
        if same && self.update.vault.is_open() {
            self.update.vault.close();
            self.update.picking = None;
        } else {
            self.update.picking = Some(machine.to_owned());
            if self.update.vault.is_open() {
                self.update.vault.reread();
            } else {
                self.update.vault.toggle();
            }
            self.read_vault_entries(cx);
        }
        cx.notify();
    }

    pub(super) fn update_vault_event(
        &mut self,
        event: PickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            PickEvent::Choose(id) => {
                let Some(machine) = self.update.picking.clone() else {
                    return;
                };
                let Some(parts) = self.feed.add_source().parts() else {
                    return;
                };
                match parts.engine.set_admin_secret(&machine, Some(&id)) {
                    Ok(()) => {
                        self.update.notice = None;
                        self.update.passwords.remove(&machine);
                        self.update.picking = None;
                        self.update.vault.close();
                        self.refresh_needs();
                    }
                    Err(why) => self.update.notice = Some(t::cluster::update_save_failed(&why)),
                }
            }
            PickEvent::Toggle | PickEvent::Clear | PickEvent::New => {
                self.update.picking = None;
                self.update.vault.close();
            }
        }
        cx.notify();
    }

    fn update_key(&mut self, machine: &str, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let modified = k.modifiers.platform || k.modifiers.control;
        let paste = k.modifiers.platform && !k.modifiers.control && k.key == "v";
        let pasted = paste
            .then(|| cx.read_from_clipboard().and_then(|c| c.text()))
            .flatten()
            .map(|t| t.chars().filter(|c| !c.is_control()).collect::<String>());
        let field = self
            .update
            .passwords
            .entry(machine.to_owned())
            .or_default()
            .field();
        let typed = match pasted {
            Some(text) => {
                field.push_str(&text);
                Typed::Changed
            }
            None => type_key(field, &k.key, k.key_char.as_deref(), modified),
        };
        match typed {
            Typed::Ignored => return,
            Typed::Changed => {}
            Typed::Submit => self.start_update(cx),
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn update_overlay(
        &mut self,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if !self.update.open {
            return None;
        }
        let rows = self.rows();
        self.sync_plan(&rows);
        let model = self.feed.snapshot();
        let running = self.update.session.running();
        let outcome = self.update.session.outcome();
        let idle = running || outcome.is_some();
        let names = self.update.offered.names();
        let sources = self.update.sources();
        let mut shown = lines(&rows, &self.update.anyway);
        shown.extend(idle_lines(&model, &rows));
        for need in &self.update.needs {
            if !self.update.focus.contains_key(&need.machine) {
                let handle = cx.focus_handle();
                self.update.focus.insert(need.machine.clone(), handle);
            }
        }
        let mut body = div().flex().flex_col().gap(px(SPACE[1])).child(label(
            t::cluster::update_intro(),
            TypeRole::Body,
            Ink::TextMuted,
            a,
        ));
        for line in &shown {
            body = body.child(self.update_card(line, &names, &sources, idle, window, a, cx));
        }
        if let Some(plan) = self.update.offered.shown() {
            let open = self.update.commands;
            body = body.child(
                div().flex().child(
                    button(
                        "update-commands",
                        if open {
                            t::cluster::add_hide_commands()
                        } else {
                            t::cluster::add_show_commands()
                        },
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.update.commands = !this.update.commands;
                        cx.notify();
                    })),
                ),
            );
            if open {
                let text = commands(plan)
                    .into_iter()
                    .map(|(title, shown)| format!("{title}\n{shown}"))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                body = body.child(mono_lines("update-commands-text", &text, 220.0, a));
            }
        }
        if running {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(SPACE[1]))
                    .child(spinner("update-busy", SPINNER_SMALL, Ink::Working, a))
                    .child(label(
                        t::cluster::update_running(),
                        TypeRole::Body,
                        Ink::TextMuted,
                        a,
                    )),
            );
        }
        if let Some(notice) = &self.update.notice {
            body = body.child(label(notice, TypeRole::Meta, Ink::Needs, a));
        }
        if let Some(outcome) = &outcome {
            body = body.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(SPACE[0]))
                    .child(icon(
                        if outcome.ok {
                            Icon::Check
                        } else {
                            Icon::Warning
                        },
                        TypeRole::Body.spec().size,
                        if outcome.ok { Ink::Done } else { Ink::Needs },
                        a,
                    ))
                    .child(div().flex_1().min_w(px(0.0)).child(label(
                        &outcome.message,
                        TypeRole::Body,
                        if outcome.ok { Ink::Text } else { Ink::Needs },
                        a,
                    ))),
            );
        }
        let footer = self.update_footer(&rows, outcome.is_some(), running, a, cx);
        let close = close_button("update-close", a, cx, |this: &mut Self, _, cx| {
            this.close_update(cx)
        });
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
                        .max_h(px(SHEET_MAX_HEIGHT))
                        .p(px(SPACE[3]))
                        .flex()
                        .flex_col()
                        .gap(px(SPACE[1]))
                        .child(card_title(
                            label(t::cluster::update_title(), TypeRole::Heading, Ink::Text, a),
                            close,
                        ))
                        .child(
                            div()
                                .id("update-body")
                                .flex_1()
                                .min_h(px(0.0))
                                .overflow_y_scroll()
                                .child(body),
                        )
                        .child(footer)
                        .part_at("Update Machines Sheet", Corner::TopTrailing, a, cx),
                ),
        )
    }

    fn update_footer(
        &mut self,
        rows: &[UpdateRow],
        done: bool,
        running: bool,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let names = self.update.offered.names();
        let planned = self.update.offered.shown().is_some();
        let blocked = self.update_blocked();
        let started = running || done;
        let reason = if started {
            None
        } else if names.is_empty() {
            let any_runnable = !selected(rows, &self.update.anyway, &BTreeSet::new()).is_empty();
            Some(if any_runnable {
                t::cluster::update_pick_one().to_owned()
            } else {
                t::cluster::update_nothing().to_owned()
            })
        } else if !planned {
            self.update.offered.error().map(str::to_owned)
        } else {
            blocked.as_ref().and_then(|b| b.reason())
        };
        let mut footer = div()
            .pt(px(SPACE[1]))
            .flex()
            .items_center()
            .gap(px(SPACE[1]))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .children(reason.map(|why| label(&why, TypeRole::Meta, Ink::TextMuted, a))),
            );
        if running {
            footer = footer.child(button(
                "update-run",
                t::cluster::update_running(),
                ButtonKind::Primary,
                PartState::Disabled,
                a,
            ));
        } else if !done && !names.is_empty() && planned {
            let can_run = blocked.is_none();
            footer = footer.child(
                button(
                    "update-run",
                    &run_label(&names),
                    ButtonKind::Primary,
                    if can_run {
                        PartState::Rest
                    } else {
                        PartState::Disabled
                    },
                    a,
                )
                .when(can_run, |b| {
                    b.on_click(cx.listener(|this, _, _, cx| this.start_update(cx)))
                }),
            );
        }
        footer
    }

    #[allow(clippy::too_many_arguments)]
    fn update_card(
        &self,
        line: &super::updates::Line,
        names: &[String],
        sources: &[(Need, Source)],
        idle: bool,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let selectable = matches!(line.kind, Kind::Ready | Kind::Chosen);
        let on = names.contains(&line.machine);
        let ink = match line.kind {
            Kind::Ready => Ink::Done,
            Kind::Chosen => Ink::AccentText,
            Kind::Busy => Ink::Needs,
            Kind::Skipped | Kind::Idle => Ink::TextFaint,
        };
        let name_ink = if matches!(line.kind, Kind::Skipped | Kind::Idle) {
            Ink::TextMuted
        } else {
            Ink::Text
        };
        let title: Div = if selectable && !idle {
            let machine = line.machine.clone();
            div().child(checkbox(
                SharedString::from(format!("update-select-{}", line.machine)),
                &line.machine,
                on,
                a,
                cx,
                move |this: &mut Self, cx| this.update_toggle(&machine, cx),
            ))
        } else {
            div().child(label(&line.machine, TypeRole::Body, name_ink, a))
        };
        let mut head = div().flex().items_center().gap(px(SPACE[1])).child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .child(title)
                .child(label(&line.text, TypeRole::Meta, ink, a)),
        );
        if line.offers_anyway() {
            let name = line.machine.clone();
            head = head.child(
                button(
                    SharedString::from(format!("update-anyway-{}", line.machine)),
                    t::cluster::update_anyway(),
                    ButtonKind::Secondary,
                    if idle {
                        PartState::Disabled
                    } else {
                        PartState::Rest
                    },
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.update_anyway(&name, cx))),
            );
        }
        let mut card_body = div().flex().flex_col().gap(px(SPACE[1])).child(head);
        if on && !idle {
            if let Some((need, source)) = sources.iter().find(|(n, _)| n.machine == line.machine) {
                card_body = card_body.child(self.update_admin(need, source, window, a, cx));
            }
        }
        card(a).p(px(SPACE[1])).child(card_body)
    }

    fn update_admin(
        &self,
        need: &Need,
        source: &Source,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let machine = need.machine.clone();
        let mut block = div().flex().flex_col().gap(px(SPACE[0]));
        let picker = |label_key: &str, text: &str, kind: ButtonKind, cx: &mut Context<Self>| {
            let machine = machine.clone();
            button(
                SharedString::from(format!("{label_key}-{machine}")),
                text,
                kind,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.update_pick(&machine, cx)))
        };
        match source {
            Source::Linked(item) => {
                block = block.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(SPACE[1]))
                        .child(icon(Icon::Check, TypeRole::Meta.spec().size, Ink::Done, a))
                        .child(div().flex_1().min_w(px(0.0)).child(label(
                            &t::cluster::update_saved_in(item),
                            TypeRole::Body,
                            Ink::Text,
                            a,
                        )))
                        .child(picker(
                            "update-change",
                            t::cluster::update_change(),
                            ButtonKind::Ghost,
                            cx,
                        )),
                );
            }
            Source::Ask(ask) => {
                block = block.child(self.update_password(need, ask, window, a, cx));
                if self.update.entries != VaultState::Unavailable {
                    block = block.child(div().flex().child(picker(
                        "update-choose",
                        t::cluster::update_choose(),
                        ButtonKind::Secondary,
                        cx,
                    )));
                }
            }
        }
        if self.update.picking.as_deref() == Some(need.machine.as_str())
            && self.update.vault.is_open()
        {
            block = block.child(pick_list(
                "update",
                &self.update.vault,
                need.link.as_deref(),
                false,
                a,
                cx,
                ClusterSheetView::update_vault_event,
            ));
        }
        block
    }

    fn update_password(
        &self,
        need: &Need,
        ask: &Ask,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut prompt = div().flex().flex_col().gap(px(SPACE[0]));
        prompt = prompt.child(label(
            &t::cluster::update_password_for(&need.machine),
            TypeRole::Body,
            Ink::Text,
            a,
        ));
        if let Some(gone) = &ask.gone {
            prompt = prompt.child(label(
                &t::cluster::update_link_gone(gone),
                TypeRole::Meta,
                Ink::Needs,
                a,
            ));
        }
        if let Some(handle) = self.update.focus.get(&need.machine) {
            let focused = handle.is_focused(window);
            let machine = need.machine.clone();
            let dots = self
                .update
                .passwords
                .get(&need.machine)
                .map(AddPassword::dots)
                .unwrap_or_default();
            let on_click_machine = need.machine.clone();
            prompt = prompt.child(
                div()
                    .id(SharedString::from(format!("update-password-{machine}")))
                    .w_full()
                    .cursor_text()
                    .track_focus(handle)
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        this.update_key(&machine, event, cx)
                    }))
                    .child(field(
                        &dots,
                        t::cluster::update_password_placeholder(),
                        focused,
                        a,
                    ))
                    .on_click(cx.listener(move |this, _, window, _| {
                        if let Some(handle) = this.update.focus.get(&on_click_machine) {
                            window.focus(handle);
                        }
                    })),
            );
        }
        let (note, ink) = match &ask.saves_as {
            Some(item) => (t::cluster::update_will_save(item), Ink::TextMuted),
            None => (t::cluster::update_vault_off().to_owned(), Ink::Needs),
        };
        prompt.child(label(&note, TypeRole::Meta, ink, a))
    }
}
