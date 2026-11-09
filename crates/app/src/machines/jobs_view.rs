//! The Jobs tab (Swift `JobCardView`, `ReadyCard`, `NeedsYouCard`, `StuckCard`): a card per
//! machine with its sd worker job, the Running switch and what the worker says about itself; then
//! what the overnight workers left for you, in Ready to merge, Needs you and Stuck. Every action
//! runs `gh`, `git` or `scp` on this Mac through the model's actions, from a click.

use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_engine::machines::forms::JobDraft;
use notchling_engine::machines::morning::{MorningEntry, MorningItem, MorningKind};
use notchling_engine::machines::registry::{Machine, SdWorkerSettings};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, label, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::forms::JobForm;
use super::model::Sheet;
use super::pieces::job_facts;
use super::view::{marked, text_line, MachinesView, CARD_PADDING};

/// A switch: a capsule with its knob at the end it's set to.
pub(super) fn switch(id: SharedString, on: bool, enabled: bool, a: Appearance) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(32.0))
        .h(px(18.0))
        .p(px(2.0))
        .rounded_full()
        .flex()
        .when(on, |s| s.justify_end())
        .fill(if on { Ink::Accent } else { Ink::Raised }, a)
        .when(enabled, |s| s.cursor_pointer())
        .when(!enabled, |s| s.opacity(0.5))
        .child(div().size(px(14.0)).rounded_full().bg(gpui::white()))
}

impl MachinesView {
    pub(super) fn jobs_tab(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let mut column = div().flex().flex_col().gap(px(24.0));
        let mut cards = div().flex().flex_col().gap(px(12.0));
        if self.model.machines.is_empty() {
            cards = cards.child(label(
                "Add a machine first.",
                TypeRole::Body,
                Ink::TextFaint,
                a,
            ));
        }
        for machine in self.model.machines.clone() {
            let shown = self.model.engine().overlaid(&machine);
            cards = cards.child(self.job_card(&shown, a, cx));
        }
        if let Some(note) = &self.model.github_note {
            cards = cards.child(marked(
                Icon::Warning,
                note,
                TypeRole::Label,
                Ink::TextMuted,
                a,
            ));
        }
        for warning in self.model.token_warnings() {
            cards = cards.child(marked(
                Icon::Warning,
                &warning,
                TypeRole::Label,
                Ink::AccentText,
                a,
            ));
        }
        column = column.child(cards);
        for (title, kind) in [
            ("Ready to merge", MorningKind::Ready),
            ("Needs you", MorningKind::NeedsYou),
            ("Stuck", MorningKind::Stuck),
        ] {
            let items: Vec<MorningItem> = self
                .model
                .items
                .iter()
                .filter(|i| i.kind == kind)
                .cloned()
                .collect();
            column = column.child(self.section(title, &items, a, cx));
        }
        column
    }

    fn section(
        &self,
        title: &str,
        items: &[MorningItem],
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut head = div().flex().items_center().gap(px(8.0)).child(label(
            title,
            TypeRole::Section,
            Ink::TextMuted,
            a,
        ));
        if !items.is_empty() {
            head = head.child(
                div()
                    .px(px(6.0))
                    .rounded_full()
                    .fill(Ink::Accent, a)
                    .child(label(
                        &items.len().to_string(),
                        TypeRole::Section,
                        Ink::OnAccent,
                        a,
                    )),
            );
        }
        let mut column = div().flex().flex_col().gap(px(10.0)).child(head);
        if items.is_empty() {
            return column.child(label("Nothing here.", TypeRole::Body, Ink::TextFaint, a));
        }
        for item in items {
            column = column.child(match &item.entry {
                MorningEntry::Ready(entry) => self.ready_card(item, entry, a, cx),
                MorningEntry::NeedsYou(entry) => self.needs_you_card(item, entry, a, cx),
                MorningEntry::Stuck(entry) => self.stuck_card(item, entry, a, cx),
            });
        }
        column
    }

    fn job_card(&self, machine: &Machine, a: Appearance, cx: &mut Context<Self>) -> Div {
        let busy = self.model.engine().running(&machine.name).is_some();
        let mut head = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(label(&machine.name, TypeRole::Title, Ink::Text, a))
            .child(label("sd worker", TypeRole::Meta, Ink::TextMuted, a))
            .child(div().flex_1());
        let name = machine.name.clone();
        let draft = JobDraft::of(machine);
        if machine.sd_worker().is_some() {
            let edit = button(
                SharedString::from(format!("edit-job-{}", machine.name)),
                "Edit…",
                ButtonKind::Secondary,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_sheet(Sheet::Job(JobForm::new(draft.clone())), window, cx)
            }));
            head = head.child(edit).child(self.running_switch(machine, a, cx));
        } else {
            let add = button(
                SharedString::from(format!("add-job-{}", machine.name)),
                "Add sd worker job…",
                ButtonKind::Primary,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_sheet(Sheet::Job(JobForm::new(JobDraft::new(&name))), window, cx)
            }));
            head = head.child(add);
        }
        let mut body = card(a)
            .w_full()
            .p(px(CARD_PADDING))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(head);
        if let Some(job) = machine.sd_worker() {
            body = body.child(settings(job, a));
        }
        if let Some(status) = self.model.status_for(machine) {
            let mut facts = div().flex().flex_col().gap(px(2.0));
            for line in job_facts(&status) {
                facts = facts.child(label(&line, TypeRole::Meta, Ink::TextMuted, a));
            }
            body = body.child(facts);
        }
        if let Some(line) = self.model.outcomes.get(&machine.name).filter(|_| !busy) {
            let ink = if line.ok {
                Ink::Done
            } else if line.guidance {
                Ink::AccentText
            } else {
                Ink::Expired
            };
            body = body.child(text_line(&line.text, TypeRole::Meta, ink, a));
        }
        body
    }

    fn running_switch(&self, machine: &Machine, a: Appearance, cx: &mut Context<Self>) -> Div {
        let sw = self.model.running_switch(machine);
        let name = machine.name.clone();
        let to = !sw.on;
        let control = switch(
            SharedString::from(format!("running-{}", machine.name)),
            sw.on,
            sw.enabled,
            a,
        )
        .when(sw.enabled, |s| {
            s.on_click(cx.listener(move |this, _, window, cx| {
                this.model.request_toggle(&name, to);
                window.focus(&this.focus);
                cx.notify();
            }))
        });
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .children(sw.note.as_deref().map(|n| {
                label(n, TypeRole::Meta, Ink::TextFaint, a)
                    .elide()
                    .max_w(px(260.0))
            }))
            .child(label("Running", TypeRole::Label, Ink::Text, a))
            .child(control)
    }
}

/// The repos, kinds, health URL and token facts of a job.
fn settings(job: &SdWorkerSettings, a: Appearance) -> Div {
    let mut column = div().flex().flex_col().gap(px(3.0));
    for repo in &job.repos {
        let name = repo.github.rsplit('/').next().unwrap_or("");
        let path = repo
            .machine_path
            .clone()
            .unwrap_or_else(|| format!("~/src/{name}"));
        column = column.child(label(
            &format!("{} → {path}", repo.github),
            TypeRole::Meta,
            Ink::Text,
            a,
        ));
    }
    let kinds = if job.kinds.is_empty() {
        "none".to_owned()
    } else {
        job.kinds.join(", ")
    };
    let health = job
        .health_url
        .as_ref()
        .map(|h| format!(" · health {h}"))
        .unwrap_or_default();
    column = column.child(label(
        &format!("kinds: {kinds}{health}"),
        TypeRole::Meta,
        Ink::TextMuted,
        a,
    ));
    if job.token_all_repos == Some(true) {
        column = column.child(label(
            "token may reach all repositories",
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        ));
    }
    if let Some(day) = &job.token_expires {
        column = column.child(label(
            &format!("token expires {day}"),
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        ));
    }
    column
}
