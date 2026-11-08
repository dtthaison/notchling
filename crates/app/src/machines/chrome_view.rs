//! The Machines window's frame (Swift `MachinesView`'s top bar and `MachinesTab`'s banners):
//! Machines | Jobs, Refresh and Add machine…; the notes and banners (machines.json unreadable or
//! migrated, Rename, Update N machines, what wasn't saved, the off message); the empty state and
//! the cards of the Machines tab.

use std::rc::Rc;

use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_kit::metric::RADIUS_MD;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, label, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::forms::AddForm;
use super::model::{Sheet, Tab};
use super::view::MachinesView;
use crate::accounts::view_rows::{segmented, Handler};
use crate::mac::gate;

impl MachinesView {
    // MARK: Top bar and banners

    pub(super) fn top_bar(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let tabs: Vec<(String, String)> = Tab::ALL
            .iter()
            .map(|t| (t.key().to_owned(), t.title().to_owned()))
            .collect();
        let pick: Handler<Self, String> = Rc::new(|this, key: String, cx| {
            this.model.tab = if key == Tab::Jobs.key() {
                Tab::Jobs
            } else {
                Tab::Machines
            };
            cx.notify();
        });
        let refresh = button(
            "refresh",
            "Refresh",
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| this.refresh_all(cx)));
        let can_add = self.model.can_add();
        let add = button(
            "add-machine",
            "Add machine…",
            ButtonKind::Primary,
            gate::state(crate::acting::Control {
                enabled: can_add,
                tooltip: None,
            }),
            a,
        );
        let add = match self.model.unreadable_note() {
            Some(note) if !can_add => gate::tip_text(add, SharedString::from(note), a),
            _ => add.on_click(cx.listener(|this, _, window, cx| {
                this.open_sheet(Sheet::Add(AddForm::new()), window, cx)
            })),
        };
        div()
            .flex_shrink_0()
            .px(px(24.0))
            .py(px(12.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .border_b(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(segmented(
                "machines-tab",
                &tabs,
                self.model.tab.key(),
                a,
                cx,
                pick,
            ))
            .child(div().flex_1())
            .child(refresh)
            .when(self.model.tab == Tab::Machines, |bar| bar.child(add))
    }

    /// A banner: an icon, plain text (it may carry a reason that came from a file or a machine),
    /// and a button on the right.
    pub(super) fn banner(
        icon: &str,
        message: &str,
        tint: Ink,
        trailing: Option<gpui::Stateful<Div>>,
        a: Appearance,
    ) -> Div {
        div()
            .w_full()
            .p(px(12.0))
            .flex()
            .items_start()
            .gap(px(10.0))
            .rounded(px(RADIUS_MD))
            .fill(Ink::AccentWash, a)
            .border(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(label(icon, TypeRole::Body, tint, a))
            .child(
                label(message, TypeRole::Body, Ink::Text, a)
                    .flex_1()
                    .min_w(px(0.0)),
            )
            .children(trailing)
    }

    fn banners(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let mut column = div().flex().flex_col().gap(px(10.0));
        if let Some(note) = self.model.unreadable_note() {
            let again = button(
                "check-again",
                "Check again",
                ButtonKind::Secondary,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(|this, _, _, cx| this.refresh_all(cx)));
            column = column
                .child(Self::banner("⚠", &note, Ink::Expired, Some(again), a))
                .child(label(
                    &self.model.registry_path(),
                    TypeRole::Mono,
                    Ink::TextMuted,
                    a,
                ));
        }
        if let Some(note) = &self.model.migration_note {
            let dismiss = button(
                "dismiss-migration",
                "Dismiss",
                ButtonKind::Ghost,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.model.dismiss_migration_note();
                cx.notify();
            }));
            column = column.child(Self::banner("↻", note, Ink::AccentText, Some(dismiss), a));
        }
        if let Some(rename) = self.model.rename_banner() {
            let go = button(
                "rename-secrets",
                "Rename",
                ButtonKind::Primary,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.model.rename_secrets();
                cx.notify();
            }));
            column = column.child(Self::banner(
                Icon::Permission.glyph(),
                &rename,
                Ink::AccentText,
                Some(go),
                a,
            ));
            if let Some(note) = &self.model.rename_note {
                column = column.child(label(note, TypeRole::Meta, Ink::Expired, a));
            }
        }
        let busy = self
            .model
            .machines
            .iter()
            .any(|m| self.model.engine().running(&m.name).is_some());
        for offer in self.model.update_offers() {
            let secret = offer.secret.clone();
            let state = if busy {
                PartState::Disabled
            } else {
                PartState::Rest
            };
            let update = button(
                SharedString::from(format!("update-{}", offer.secret)),
                &offer.title(),
                ButtonKind::Primary,
                state,
                a,
            )
            .when(!busy, |b| {
                b.on_click(cx.listener(move |this, _, window, cx| {
                    this.model.request_update(&secret);
                    window.focus(&this.focus);
                    cx.notify();
                }))
            });
            column = column.child(Self::banner(
                "↑",
                &format!("{} has a new value.", offer.secret),
                Ink::AccentText,
                Some(update),
                a,
            ));
            if let Some(note) = self.model.update_notes.get(&offer.secret) {
                column = column.child(label(note, TypeRole::Meta, Ink::Expired, a));
            }
        }
        if let Some(problem) = &self.model.registry_problem {
            column = column.child(label(
                &format!("⚠ {problem}"),
                TypeRole::Label,
                Ink::Expired,
                a,
            ));
        }
        if self.model.engine().save_refused() {
            column = column.child(label(
                "⚠ What the checks found was not saved: machines.json can't be written.",
                TypeRole::Label,
                Ink::TextMuted,
                a,
            ));
        }
        if let Some(off) = self.model.parts.off {
            column = column.child(label(off, TypeRole::Meta, Ink::TextFaint, a));
        }
        if let Some(notice) = &self.model.notice {
            column = column.child(label(notice, TypeRole::Meta, Ink::Expired, a));
        }
        column
    }

    fn empty(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let can_add = self.model.can_add();
        let add = button(
            "add-first-machine",
            "Add machine…",
            ButtonKind::Primary,
            if can_add {
                PartState::Rest
            } else {
                PartState::Disabled
            },
            a,
        )
        .when(can_add, |b| {
            b.on_click(cx.listener(|this, _, window, cx| {
                this.open_sheet(Sheet::Add(AddForm::new()), window, cx)
            }))
        });
        div()
            .max_w(px(560.0))
            .py(px(12.0))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(label("No machines yet", TypeRole::Title, Ink::Text, a))
            .child(label(
                "Machines are the Macs and servers that do Notchling's overnight jobs. Add one by its Tailscale name; Notchling checks it and lists what it still needs.",
                TypeRole::Body,
                Ink::TextMuted,
                a,
            ))
            .child(label(&self.model.registry_path(), TypeRole::Mono, Ink::TextFaint, a))
            .child(div().child(add))
    }

    pub(super) fn machines_tab(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let mut column = div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(self.banners(a, cx));
        if self.model.machines.is_empty() {
            column = column.child(self.empty(a, cx));
        } else {
            let machines = self.model.machines.clone();
            for machine in &machines {
                column = column.child(self.machine_card(machine, a, cx));
            }
        }
        column
    }
}
