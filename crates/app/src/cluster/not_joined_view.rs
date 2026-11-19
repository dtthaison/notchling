use std::rc::Rc;

use gpui::{deferred, div, prelude::*, px, Context, Div, SharedString};
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, icon_button, label, menu_panel, KitPart, KitStyled, MenuItem,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::add_sheet::{brief, not_joined_line};
use super::machine_add::remove;
use super::model::ClusterModel;
use super::not_joined::{checked_line, NotJoined, RouteLine};
use super::view::ClusterSheetView;
use crate::accounts::flows::Alert;
use crate::accounts::view_rows::{alert_overlay, Handler};
use crate::mac::gate;
use crate::machines::data::now_secs;

impl ClusterSheetView {
    pub(super) fn add_rows(&self, a: Appearance, cx: &mut Context<Self>) -> Vec<Div> {
        let runs = self.feed.adds();
        runs.briefs()
            .into_iter()
            .filter_map(|(name, phase)| brief(&name, &phase))
            .map(|row| {
                let mut line = div().flex().items_center().gap(px(SPACE[1])).child(
                    div().flex_1().min_w(px(0.0)).child(label(
                        &row.text,
                        TypeRole::Meta,
                        if row.problem {
                            Ink::Needs
                        } else {
                            Ink::TextMuted
                        },
                        a,
                    )),
                );
                let show = row.name.clone();
                line = line.child(
                    button(
                        SharedString::from(format!("add-row-show-{}", row.name)),
                        t::cluster::add_row_show(),
                        ButtonKind::Secondary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.show_add(&show, cx))),
                );
                if row.dismiss {
                    let name = row.name.clone();
                    line = line.child(
                        button(
                            SharedString::from(format!("add-row-dismiss-{}", row.name)),
                            t::cluster::add_row_dismiss(),
                            ButtonKind::Ghost,
                            PartState::Rest,
                            a,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.feed.adds().dismiss(&name);
                            cx.notify();
                        })),
                    );
                }
                line
            })
            .collect()
    }

    pub(super) fn not_joined_block(
        &mut self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let list = self.not_joined_list(model);
        if list.is_empty() {
            return None;
        }
        let can_join = model.accepts_machines();
        let mut block = div().flex().flex_col().gap(px(SPACE[1])).child(label(
            t::cluster::not_joined_title(),
            TypeRole::Label,
            Ink::TextFaint,
            a,
        ));
        for machine in &list {
            block = block.child(self.not_joined_row(machine, can_join, a, cx));
        }
        Some(block.part("Not in the Cluster", a, cx))
    }

    fn machine_menu_id(name: &str) -> String {
        format!("machine:{name}")
    }

    pub(super) fn check_machine(&mut self, name: &str, cx: &mut Context<Self>) {
        if !self.checking.insert(name.to_owned()) {
            return;
        }
        let source = self.feed.add_source();
        let name = name.to_owned();
        cx.spawn(async move |this, cx| {
            let asked = name.clone();
            cx.background_executor()
                .spawn(async move { source.check(&asked) })
                .await;
            this.update(cx, |view, cx| {
                view.checking.remove(&name);
                view.add.forget_list();
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn remove_machine(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(parts) = self.feed.add_source().parts() else {
            return;
        };
        let runs = self.feed.adds();
        if runs.busy(name) {
            self.note = Some((t::cluster::machine_remove_busy(name), true));
            cx.notify();
            return;
        }
        let model = self.feed.snapshot();
        self.note = match remove(&parts, &model, name) {
            Ok(()) => {
                runs.dismiss(name);
                self.feed.add_source().forget(name);
                None
            }
            Err(why) => Some((why, true)),
        };
        self.add.forget_list();
        cx.notify();
    }

    pub(super) fn machine_remove_alert(
        &self,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let name = self.removing_machine.as_ref()?;
        let alert = Alert {
            title: t::cluster::machine_remove_title().to_owned(),
            message: t::cluster::machine_remove_note(name),
            button: t::cluster::machine_remove_confirm().to_owned(),
            destructive: true,
        };
        let on: Handler<Self, bool> = Rc::new(|this, yes, cx| {
            let Some(name) = this.removing_machine.take() else {
                return;
            };
            if yes {
                this.remove_machine(&name, cx);
            } else {
                cx.notify();
            }
        });
        Some(alert_overlay(&alert, false, a, cx, on))
    }

    fn machine_menu(&self, name: &str, a: Appearance, cx: &mut Context<Self>) -> Div {
        let remove = name.to_owned();
        let edit = name.to_owned();
        let items = menu_panel(a)
            .child(
                MenuItem::new(
                    SharedString::from(format!("machine-edit-addresses-{name}")),
                    t::cluster::machine_edit_addresses(),
                )
                .build(a)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.menu = None;
                    this.open_edit_addresses(&edit, cx);
                })),
            )
            .child(
                MenuItem::new(
                    SharedString::from(format!("machine-remove-{name}")),
                    t::cluster::machine_remove(),
                )
                .build(a)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.menu = None;
                    this.removing_machine = Some(remove.clone());
                    cx.notify();
                })),
            );
        div()
            .absolute()
            .right(px(SPACE[1]))
            .top(px(SPACE[4] + SPACE[1]))
            .child(
                div()
                    .id(SharedString::from(format!("machine-click-away-{name}")))
                    .absolute()
                    .top(px(-4000.0))
                    .left(px(-4000.0))
                    .size(px(8000.0))
                    .occlude()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.menu = None;
                        cx.notify();
                    })),
            )
            .child(items)
    }

    fn not_joined_row(
        &self,
        machine: &NotJoined,
        can_join: bool,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let busy = self.busy;
        let checking = self.checking.contains(&machine.name);
        let now_s = self.now.map_or_else(|| now_secs() as i64, |ms| ms / 1000);
        let status = if checking {
            t::cluster::machine_checking().to_owned()
        } else {
            checked_line(machine, now_s)
        };
        let ink = match machine.reach {
            super::not_joined::Reach::Down => Ink::Needs,
            _ => Ink::TextMuted,
        };
        let route_text = match &machine.route {
            RouteLine::Hidden => None,
            RouteLine::Via(text) => Some(text.clone()),
            RouteLine::Wrong { wanted, active } => {
                Some(t::cluster::machine_wrong_network(wanted, active))
            }
            RouteLine::NoAnswer => Some(t::cluster::machine_no_address().to_owned()),
        };
        let route_ink = match &machine.route {
            RouteLine::Wrong { .. } | RouteLine::NoAnswer => Ink::Needs,
            _ => Ink::TextMuted,
        };
        let switch = match &machine.route {
            RouteLine::Wrong { wanted, active } => {
                let (name, wanted, active) = (machine.name.clone(), wanted.clone(), active.clone());
                Some(
                    button(
                        super::edit_addresses_view::switch_button_id(&machine.name),
                        t::cluster::machine_switch_network(),
                        ButtonKind::Secondary,
                        if busy {
                            PartState::Disabled
                        } else {
                            PartState::Rest
                        },
                        a,
                    )
                    .when(!busy, |b| {
                        b.on_click(cx.listener(move |this, _, _, cx| {
                            this.ask_switch(&name, &wanted, &active, cx)
                        }))
                    }),
                )
            }
            _ => None,
        };
        let check_name = machine.name.clone();
        let check = button(
            SharedString::from(format!("machine-check-{}", machine.name)),
            t::cluster::machine_check(),
            ButtonKind::Secondary,
            if checking {
                PartState::Disabled
            } else {
                PartState::Rest
            },
            a,
        )
        .when(!checking, |b| {
            b.on_click(cx.listener(move |this, _, _, cx| this.check_machine(&check_name, cx)))
        });
        let join_name = machine.name.clone();
        let join = button(
            SharedString::from(format!("not-joined-join-{}", machine.name)),
            t::cluster::not_joined_join(),
            ButtonKind::Secondary,
            if busy {
                PartState::Disabled
            } else {
                PartState::Rest
            },
            a,
        )
        .when(!busy, |b| {
            b.on_click(cx.listener(move |this, _, _, cx| this.open_join(&join_name, cx)))
        });
        let menu_id = Self::machine_menu_id(&machine.name);
        let open = self.menu.as_deref() == Some(menu_id.as_str());
        let more = icon_button(
            SharedString::from(format!("machine-more-{}", machine.name)),
            Icon::More,
            if open {
                ButtonKind::Secondary
            } else {
                ButtonKind::Ghost
            },
            PartState::Rest,
            a,
        );
        let toggle = menu_id.clone();
        let more = gate::tip_text(more, t::cluster::more().into(), a)
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_menu(&toggle, cx)));
        let menu = open.then(|| deferred(self.machine_menu(&machine.name, a, cx)).with_priority(1));
        card(a)
            .relative()
            .p(px(SPACE[2]))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(SPACE[1]))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap(px(SPACE[0] / 2.0))
                            .child(label(&machine.name, TypeRole::Heading, Ink::Text, a).elide())
                            .child(label(
                                &not_joined_line(machine),
                                TypeRole::Meta,
                                Ink::TextMuted,
                                a,
                            ))
                            .child(label(&status, TypeRole::Meta, ink, a))
                            .children(
                                route_text.map(|text| label(&text, TypeRole::Meta, route_ink, a)),
                            ),
                    )
                    .children(switch)
                    .child(check)
                    .children(can_join.then_some(join))
                    .child(more),
            )
            .children(menu)
    }
}
