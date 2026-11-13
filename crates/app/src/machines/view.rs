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
use notchling_kit::metric::{SIDE_PADDING, SPINNER_MINI, SPINNER_SMALL};
use notchling_kit::ui::{
    icon, label, resize_edges, scrollable, spinner as kit_spinner, window_appearance, KitStyled,
    Scrollable, TopBorder,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::forms::AddForm;
use super::model::{MachinesModel, Parts, Run, Sheet, Tab};
use super::renders;
use crate::notch::answer::{type_key, Typed};

const TICK: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, PartialEq)]
pub enum Confirming {
    Remove(String),
    Close(MorningItem),
    Drop(MorningItem),
}

pub struct MachinesView {
    pub model: MachinesModel,
    pub(super) focus: FocusHandle,
    pub(super) typing: Option<String>,
    pub(super) notes: BTreeMap<String, String>,
    pub(super) sending_back: BTreeSet<String>,
    pub(super) confirming: Option<Confirming>,
    pub(super) log_open: BTreeSet<String>,
    pub(super) how_to: BTreeMap<String, StepId>,
    seen_revision: u64,
    fixed: bool,
}

impl MachinesView {
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
                    let vault = view.model.watch_vault();
                    if asked || vault || revision != view.seen_revision {
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

    pub(super) fn refresh_all(&mut self, cx: &mut Context<Self>) {
        self.reload(cx);
        if self.fixed {
            return;
        }
        let engine = self.model.parts.engine.clone();
        Self::background(cx, move || engine.refresh(None, true), |_, _, _| {});
    }

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
            |view, _, cx| {
                view.reload(cx);
                view.check_after_poll(cx);
            },
        );
    }

    fn check_after_poll(&mut self, cx: &mut Context<Self>) {
        if self.fixed || !self.model.check_after_poll() {
            return;
        }
        let engine = self.model.parts.engine.clone();
        Self::background(
            cx,
            move || engine.refresh(None, false),
            |view, _, _| view.model.checked(),
        );
    }

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

    pub(super) fn open_sheet(&mut self, sheet: Sheet, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.open_sheet(sheet) {
            window.focus(&self.focus);
        }
        cx.notify();
    }

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
            .child(scrollable(
                div()
                    .id("machines-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .p(px(24.0))
                    .child(body),
            ))
            .children(self.sheet_overlay(a, cx))
            .children(self.confirm_overlay(a, cx))
            .children(self.diff_overlay(a, cx))
            .child(resize_edges(window, TopBorder::InTitleBar))
    }
}

pub fn build_render(name: &str, cx: &mut App) -> Option<AnyView> {
    let which = renders::shown(name)?;
    let model = renders::model(which);
    Some(cx.new(|cx| MachinesView::fixed(model, cx)).into())
}

pub(super) fn state_mark(
    state: Option<&notchling_engine::machines::step::CheckState>,
) -> (Icon, Ink) {
    use notchling_engine::machines::step::CheckState;
    match state {
        Some(CheckState::Pass(_)) => (Icon::Done, Ink::Done),
        Some(CheckState::Fail(_)) => (Icon::Fail, Ink::Expired),
        Some(CheckState::CantTell(_)) => (Icon::Unsure, Ink::TextFaint),
        None => (Icon::Working, Ink::TextFaint),
    }
}

pub(super) fn state_icon(
    key: impl Into<SharedString>,
    mark: Icon,
    ink: Ink,
    role: TypeRole,
    a: Appearance,
) -> Div {
    if mark == Icon::Working {
        kit_spinner(key.into(), SPINNER_MINI, ink, a)
    } else {
        icon(mark, role.spec().size, ink, a)
    }
}

pub(super) fn busy_line(
    key: impl Into<SharedString>,
    text: &str,
    role: TypeRole,
    ink: Ink,
    a: Appearance,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(kit_spinner(key.into(), SPINNER_SMALL, Ink::Working, a))
        .child(label(text, role, ink, a))
}

pub(super) fn marked(mark: Icon, text: &str, role: TypeRole, ink: Ink, a: Appearance) -> Div {
    div()
        .w_full()
        .min_w(px(0.0))
        .flex()
        .items_start()
        .gap(px(6.0))
        .child(icon(mark, role.spec().size, ink, a))
        .child(label(text, role, ink, a).flex_1().min_w(px(0.0)))
}

pub(super) fn mono_box(
    id: impl Into<SharedString>,
    text: &str,
    max_h: f32,
    a: Appearance,
) -> Scrollable {
    scrollable(mono_lines(id, text, max_h, a))
}

pub(super) fn mono_lines(
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

pub(super) fn text_line(text: &str, role: TypeRole, ink: Ink, a: Appearance) -> Div {
    label(text, role, ink, a).w_full().min_w(px(0.0))
}

pub(super) const CARD_PADDING: f32 = SIDE_PADDING;
