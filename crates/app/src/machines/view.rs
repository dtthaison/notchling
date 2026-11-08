//! The Machines window over GPUI (macOS): Machines | Jobs, Refresh and Add machine… at the top,
//! the banners, then the cards (`cards`, `jobs_view`) and the sheets over them (`sheets_view`).
//! Everything that touches a machine runs on the background executor through the model's parts,
//! and only from a click: a Fix after its plan was shown in the Fix sheet and Run was pressed.
//! The admin password lives in the model only while its Fix sheet is on show (`model_sheet`): every
//! change of sheet clears it, and it never reaches a log.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use gpui::{
    div, prelude::*, px, AnyView, App, Context, Div, FocusHandle, KeyDownEvent, Render,
    SharedString, Window,
};
use notchling_engine::machines::actions::MorningActions;
use notchling_engine::machines::morning::MorningItem;
use notchling_engine::machines::poller::INTERVAL_SECS;
use notchling_engine::machines::step::StepId;
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::ui::{label, window_appearance, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::forms::AddForm;
use super::model::{MachinesModel, Parts, Run, Sheet, Tab};
use super::renders;
use crate::notch::answer::{type_key, Typed};

/// How often the view looks for what changed off the UI thread.
const TICK: Duration = Duration::from_millis(500);

/// What an alert asks before a click goes through.
#[derive(Clone, Debug, PartialEq)]
pub enum Confirming {
    Remove(String),
    Close(MorningItem),
    Drop(MorningItem),
}

pub struct MachinesView {
    pub model: MachinesModel,
    pub(super) focus: FocusHandle,
    /// The morning card whose note field takes the keys.
    pub(super) typing: Option<String>,
    /// The Send back and Answer texts, by item id.
    pub(super) notes: BTreeMap<String, String>,
    pub(super) sending_back: BTreeSet<String>,
    pub(super) confirming: Option<Confirming>,
    pub(super) log_open: BTreeSet<String>,
    pub(super) how_to: BTreeMap<String, StepId>,
    seen_revision: u64,
    /// A render: nothing polls, nothing is sent.
    fixed: bool,
}

impl MachinesView {
    /// The live window: reads the registry, fetches the issues, checks every machine (the slow
    /// probes too), polls the workers every minute and watches the engine for changes.
    pub fn live(parts: Parts, cx: &mut Context<Self>) -> Self {
        let mut view = MachinesView::over(MachinesModel::new(parts), false, cx);
        view.refresh_all(cx);
        view.poll(cx);
        cx.spawn(async move |this, cx| {
            let mut ticks: u64 = 0;
            loop {
                cx.background_executor().timer(TICK).await;
                ticks += 1;
                let poll_due = ticks.is_multiple_of(INTERVAL_SECS * 2);
                let alive = this.update(cx, |view, cx| {
                    let revision = view.model.engine().revision();
                    let asked = view.model.tick();
                    if asked || revision != view.seen_revision {
                        view.seen_revision = revision;
                        cx.notify();
                    }
                    if poll_due && !view.model.machines.is_empty() {
                        view.poll(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        view
    }

    /// A render's: the model as given, nothing running.
    pub fn fixed(model: MachinesModel, cx: &mut Context<Self>) -> Self {
        MachinesView::over(model, true, cx)
    }

    fn over(model: MachinesModel, fixed: bool, cx: &mut Context<Self>) -> Self {
        MachinesView {
            seen_revision: model.engine().revision(),
            model,
            focus: cx.focus_handle(),
            typing: None,
            notes: BTreeMap::new(),
            sending_back: BTreeSet::new(),
            confirming: None,
            log_open: BTreeSet::new(),
            how_to: BTreeMap::new(),
            fixed,
        }
    }

    // MARK: Background work

    /// Runs `work` on the background executor, then `done` with its result on the UI thread.
    pub(super) fn background<T: Send + 'static>(
        cx: &mut Context<Self>,
        work: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        cx.spawn(async move |this, cx| {
            let out = cx.background_executor().spawn(async move { work() }).await;
            this.update(cx, |view, cx| {
                done(view, out, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The registry and the vault again, the items from the latest reports, and the issues.
    pub(super) fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(fetch) = self.model.reload() else {
            return;
        };
        if self.fixed {
            return;
        }
        let runner = self.model.parts.runner.clone();
        let generation = fetch.generation;
        Self::background(
            cx,
            move || fetch.run(&*runner),
            move |view, result, _| view.model.issues_fetched(generation, result),
        );
    }

    /// Refresh: the registry, the issues, and every machine's checks, the slow ones too.
    pub(super) fn refresh_all(&mut self, cx: &mut Context<Self>) {
        self.reload(cx);
        if self.fixed {
            return;
        }
        let engine = self.model.parts.engine.clone();
        Self::background(cx, move || engine.refresh(None, true), |_, _, _| {});
    }

    /// A card's Check.
    pub(super) fn check(&mut self, machine: &str, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        let engine = self.model.parts.engine.clone();
        let names = vec![machine.to_owned()];
        Self::background(cx, move || engine.refresh(Some(&names), true), |_, _, _| {});
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        let poller = self.model.parts.poller.clone();
        Self::background(
            cx,
            move || poller.refresh(false),
            |view, _, cx| view.reload(cx),
        );
    }

    /// Rename: the legacy secrets move through the vault off the UI thread.
    pub(super) fn rename_secrets(&mut self, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        let Some(work) = self.model.rename_work() else {
            return;
        };
        Self::background(
            cx,
            move || work.run(),
            |view, failed, _| view.model.renamed(failed),
        );
    }

    /// Renew…: the page opens off the UI thread.
    pub(super) fn renew(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        let work = self.model.renew_work(name);
        Self::background(
            cx,
            move || work.run(),
            |view, result, _| view.model.renewed(result),
        );
    }

    /// A confirmed plan runs; its outcome lands on the card.
    fn run(&mut self, run: Run, cx: &mut Context<Self>) {
        if self.fixed {
            return;
        }
        let engine = self.model.parts.engine.clone();
        Self::background(
            cx,
            move || run.execute(&engine),
            |view, (done, outcome), _| view.model.finished(&done, &outcome),
        );
    }

    /// An action on a morning item, then a poll and a reload; `success` is the line it leaves.
    pub(super) fn perform(
        &mut self,
        item: &MorningItem,
        success: Option<&'static str>,
        action: impl FnOnce(&MorningActions) -> bool + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.fixed || !self.model.may_act(item) {
            return;
        }
        let actions = self.model.parts.actions.clone();
        let poller = self.model.parts.poller.clone();
        let id = item.id.clone();
        Self::background(
            cx,
            move || {
                action(&actions);
                poller.refresh(false);
            },
            move |view, _, cx| {
                view.model.acted(&id, success);
                view.reload(cx);
            },
        );
        cx.notify();
    }

    pub(super) fn show_diff(&mut self, item: &MorningItem, cx: &mut Context<Self>) {
        self.model.show_diff(item);
        if self.fixed {
            return;
        }
        let actions = self.model.parts.actions.clone();
        let item = item.clone();
        let id = item.id.clone();
        Self::background(
            cx,
            move || actions.diff(&item),
            move |view, text, _| view.model.diff_loaded(&id, text),
        );
    }

    // MARK: Sheets

    /// An Add or Job sheet; refused while another sheet is on show.
    pub(super) fn open_sheet(&mut self, sheet: Sheet, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.open_sheet(sheet) {
            window.focus(&self.focus);
        }
        cx.notify();
    }

    /// The Fix sheet's Run: the password, if any, goes into this one confirm and nowhere else.
    pub(super) fn confirm_sheet(&mut self, cx: &mut Context<Self>) {
        if let Some(run) = self.model.confirm() {
            self.run(run, cx);
        }
        cx.notify();
    }

    pub(super) fn cancel_sheet(&mut self, cx: &mut Context<Self>) {
        self.model.cancel_sheet();
        cx.notify();
    }

    /// The Add sheet's Check, off the UI thread; the result lands in the form if its fields are
    /// still the same.
    pub(super) fn check_draft(&mut self, cx: &mut Context<Self>) {
        let existing = self.model.names();
        let can_add = self.model.can_add();
        let Some(Sheet::Add(form)) = self.model.sheet_mut() else {
            return;
        };
        if !form.can_check(&existing, can_add) {
            return;
        }
        form.checking = true;
        let (name, host, user) = (form.name.clone(), form.host.clone(), form.user.clone());
        let engine = self.model.parts.engine.clone();
        Self::background(
            cx,
            move || MachinesModel::draft_check(&engine, &name, &host, &user, &existing),
            |view, check, _| {
                if let Some(Sheet::Add(form)) = view.model.sheet_mut() {
                    form.checked(check);
                }
            },
        );
        cx.notify();
    }

    /// The Add sheet's Add: the record, then the new machine's checks.
    pub(super) fn add_machine(&mut self, cx: &mut Context<Self>) {
        let existing = self.model.names();
        let can_add = self.model.can_add();
        let Some(Sheet::Add(form)) = self.model.sheet() else {
            return;
        };
        if !form.can_add(&existing, can_add) {
            return;
        }
        let form: AddForm = form.clone();
        let Some(check) = &form.check else {
            return;
        };
        match self.model.add(&form.name, &form.host, &form.user, check) {
            Ok(()) => {
                self.model.close_sheet();
                self.check(&form.name, cx);
            }
            Err(why) => {
                if let Some(Sheet::Add(form)) = self.model.sheet_mut() {
                    form.error = Some(why);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn save_job(&mut self, cx: &mut Context<Self>) {
        let Some(Sheet::Job(form)) = self.model.sheet() else {
            return;
        };
        if !form.can_save() {
            return;
        }
        let draft = form.draft.clone();
        match self.model.save_job(&draft) {
            Ok(()) => self.model.close_sheet(),
            Err(why) => {
                if let Some(Sheet::Job(form)) = self.model.sheet_mut() {
                    form.error = Some(why);
                }
            }
        }
        cx.notify();
    }

    // MARK: Keys

    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let modified = k.modifiers.platform || k.modifiers.control;
        let paste = k.modifiers.platform && !k.modifiers.control && k.key == "v";
        let pasted = paste
            .then(|| cx.read_from_clipboard().and_then(|c| c.text()))
            .flatten()
            .map(|t| t.chars().filter(|c| !c.is_control()).collect::<String>());
        if k.key == "escape" {
            if self.confirming.take().is_some() || self.model.diff.take().is_some() {
            } else if self.model.sheet().is_some() {
                self.cancel_sheet(cx);
            } else {
                self.typing = None;
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let typed = if let Some(password) = self.model.password_field() {
            match pasted {
                Some(text) => {
                    password.push_str(&text);
                    Typed::Changed
                }
                None => type_key(password, &k.key, k.key_char.as_deref(), modified),
            }
        } else {
            self.type_into_sheet(pasted, &k.key, k.key_char.as_deref(), modified)
        };
        match typed {
            Typed::Ignored => return,
            Typed::Changed => {}
            Typed::Submit => self.submit(cx),
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// A key for an Add or Job sheet's field, or for the note field under no sheet.
    fn type_into_sheet(
        &mut self,
        pasted: Option<String>,
        key: &str,
        key_char: Option<&str>,
        modified: bool,
    ) -> Typed {
        match self.model.sheet_mut() {
            Some(Sheet::Fix(_)) => Typed::Ignored,
            Some(Sheet::Add(form)) => match pasted {
                Some(text) => {
                    form.paste(&text);
                    Typed::Changed
                }
                None => form.type_key(key, key_char, modified),
            },
            Some(Sheet::Job(form)) => match pasted {
                Some(text) => {
                    form.paste(&text);
                    Typed::Changed
                }
                None => form.type_key(key, key_char, modified),
            },
            None => match self.typing.clone() {
                Some(id) => {
                    let note = self.notes.entry(id).or_default();
                    match pasted {
                        Some(text) => {
                            note.push_str(&text);
                            Typed::Changed
                        }
                        None => type_key(note, key, key_char, modified),
                    }
                }
                None => Typed::Ignored,
            },
        }
    }

    /// Return: the sheet's default button, or the note field's Send.
    fn submit(&mut self, cx: &mut Context<Self>) {
        let existing = self.model.names();
        let can_add = self.model.can_add();
        let password_given = self.model.password_len() > 0;
        match self.model.sheet() {
            Some(Sheet::Fix(prompt)) => {
                if prompt.can_run(password_given) {
                    self.confirm_sheet(cx);
                }
            }
            Some(Sheet::Add(form)) => {
                if form.can_add(&existing, can_add) {
                    self.add_machine(cx);
                } else if form.can_check(&existing, can_add) {
                    self.check_draft(cx);
                }
            }
            Some(Sheet::Job(_)) => self.save_job(cx),
            None => {
                if let Some(id) = self.typing.clone() {
                    self.send_note(&id, cx);
                }
            }
        }
    }
}

impl Render for MachinesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        // the sheets and the note fields type through this view
        if (self.model.sheet().is_some() || self.typing.is_some()) && !self.focus.is_focused(window)
        {
            window.focus(&self.focus);
        }
        let body = match self.model.tab {
            Tab::Machines => self.machines_tab(a, cx),
            Tab::Jobs => self.jobs_tab(a, cx),
        };
        div()
            .id("machines-window")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key(event, cx)))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .fill(Ink::Window, a)
            .child(self.top_bar(a, cx))
            .child(
                div()
                    .id("machines-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .p(px(24.0))
                    .child(body),
            )
            .children(self.sheet_overlay(a, cx))
            .children(self.confirm_overlay(a, cx))
            .children(self.diff_overlay(a, cx))
    }
}

/// `--render machines|machines-fix|machines-add|machines-jobs|machines-job-sheet`, over the
/// fixture with the tool off; nothing runs.
pub fn build_render(name: &str, cx: &mut App) -> Option<AnyView> {
    let which = renders::shown(name)?;
    let model = renders::model(which);
    Some(cx.new(|cx| MachinesView::fixed(model, cx)).into())
}

/// Shared by the cards: the step state's glyph and ink.
pub(super) fn state_mark(
    state: Option<&notchling_engine::machines::step::CheckState>,
) -> (&'static str, Ink) {
    use notchling_engine::machines::step::CheckState;
    match state {
        Some(CheckState::Pass(_)) => (Icon::Check.glyph(), Ink::Done),
        Some(CheckState::Fail(_)) => ("✗", Ink::Expired),
        Some(CheckState::CantTell(_)) => ("?", Ink::TextFaint),
        None => ("◌", Ink::TextFaint),
    }
}

/// A box of monospaced lines on the sidebar's fill.
pub(super) fn mono_box(
    id: impl Into<SharedString>,
    text: &str,
    max_h: f32,
    a: Appearance,
) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .w_full()
        .max_h(px(max_h))
        .overflow_y_scroll()
        .p(px(10.0))
        .rounded(px(notchling_kit::metric::RADIUS_SM))
        .fill(Ink::Sidebar, a)
        .child(label(text, TypeRole::Mono, Ink::Text, a))
}

/// A plain-text line that may be long: wraps, never clipped.
pub(super) fn text_line(text: &str, role: TypeRole, ink: Ink, a: Appearance) -> Div {
    label(text, role, ink, a).w_full().min_w(px(0.0))
}

/// The gap the window keeps around a card's body.
pub(super) const CARD_PADDING: f32 = SIDE_PADDING;
