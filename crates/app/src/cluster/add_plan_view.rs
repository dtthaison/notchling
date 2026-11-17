use gpui::{div, prelude::*, px, Context, Div, KeyDownEvent, SharedString, Window};
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, field, icon, label, KitPart};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::add_plan::{Choice, PlanRow, PlanView};
use super::add_runs::Snap;
use super::view::ClusterSheetView;
use crate::machines::sheets_view::checkbox;
use crate::machines::view::mono_lines;

impl ClusterSheetView {
    pub(super) fn plan_block(
        &self,
        plan: &PlanView,
        snap: Option<&Snap>,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let host = snap.map(|s| s.host.as_str()).unwrap_or_default();
        let target = snap
            .map(|s| format!("{}@{}", s.user, s.host))
            .unwrap_or_default();
        let mut list = div().flex().flex_col().gap(px(SPACE[1]));
        for row in &plan.rows {
            list = list.child(self.plan_row(row, a, cx));
        }
        list = list.child(self.choices(plan, a, cx));
        if let Some(sheet) = &self.add.sheet {
            if plan.needs_password {
                let focused = sheet.focus.is_focused(window);
                list =
                    list.child(
                        div()
                            .id("add-password")
                            .w_full()
                            .cursor_text()
                            .track_focus(&sheet.focus)
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                                this.add_key(event, cx)
                            }))
                            .child(field(
                                &sheet.password.dots(),
                                &t::cluster::add_password_placeholder(&target),
                                focused,
                                a,
                            ))
                            .on_click(cx.listener(|this, _, window, _| {
                                if let Some(sheet) = &this.add.sheet {
                                    window.focus(&sheet.focus);
                                }
                            })),
                    );
            }
        }
        if plan.root_confirm {
            list = list.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(SPACE[0]))
                    .child(icon(
                        Icon::Warning,
                        TypeRole::Meta.spec().size,
                        Ink::AccentText,
                        a,
                    ))
                    .child(div().flex_1().min_w(px(0.0)).child(label(
                        &t::cluster::add_root_warning(host),
                        TypeRole::Meta,
                        Ink::AccentText,
                        a,
                    ))),
            );
        }
        if let Some(blocked) = &plan.blocked {
            list = list.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(SPACE[0]))
                    .child(icon(
                        Icon::Warning,
                        TypeRole::Meta.spec().size,
                        Ink::Needs,
                        a,
                    ))
                    .child(div().flex_1().min_w(px(0.0)).child(label(
                        blocked,
                        TypeRole::Meta,
                        Ink::Needs,
                        a,
                    ))),
            );
        }
        list.part("Machine Plan", a, cx)
    }

    fn plan_row(&self, row: &PlanRow, a: Appearance, cx: &mut Context<Self>) -> Div {
        let id = row.id.raw().to_owned();
        let open = self
            .add
            .sheet
            .as_ref()
            .is_some_and(|s| s.commands.contains(&id));
        let (glyph, ink) = if row.waiting {
            (Icon::Clock, Ink::TextFaint)
        } else {
            (Icon::Warning, Ink::Needs)
        };
        let mut head = div()
            .flex()
            .items_center()
            .gap(px(SPACE[1]))
            .child(icon(glyph, TypeRole::Body.spec().size, ink, a))
            .child(div().flex_1().min_w(px(0.0)).child(label(
                &row.title,
                TypeRole::Body,
                Ink::Text,
                a,
            )));
        if row.admin {
            head = head.child(Self::pill(
                t::cluster::add_admin_tag(),
                Ink::AccentText,
                Ink::AccentWash,
                a,
            ));
        }
        if row.howto {
            head = head.child(Self::pill(
                t::cluster::add_howto_tag(),
                Ink::TextMuted,
                Ink::Raised,
                a,
            ));
        }
        if !row.howto {
            let toggle = id.clone();
            head = head.child(
                button(
                    SharedString::from(format!("add-commands-{id}")),
                    if open {
                        t::cluster::add_hide_commands()
                    } else {
                        t::cluster::add_show_commands()
                    },
                    ButtonKind::Ghost,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.add_toggle_commands(&toggle, cx))),
            );
        }
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(SPACE[0]))
            .child(head)
            .child(label(&row.why, TypeRole::Meta, Ink::TextMuted, a));
        if !row.reason.is_empty() {
            body = body.child(label(&row.reason, TypeRole::Meta, Ink::TextFaint, a));
        }
        if row.howto {
            body = body.child(mono_lines(
                SharedString::from(format!("add-howto-{id}")),
                &row.display,
                140.0,
                a,
            ));
        } else if open {
            body = body.child(mono_lines(
                SharedString::from(format!("add-commands-text-{id}")),
                &row.display,
                200.0,
                a,
            ));
        }
        card(a).p(px(SPACE[2])).child(body)
    }

    fn choice_row(
        &self,
        id: &'static str,
        title: &str,
        choice: &Choice,
        a: Appearance,
        cx: &mut Context<Self>,
        set: fn(&mut Self, bool, &mut Context<Self>),
    ) -> Div {
        if choice.editable {
            let on = choice.on;
            return div().child(checkbox(
                SharedString::from(id),
                title,
                on,
                a,
                cx,
                move |this: &mut Self, cx| set(this, !on, cx),
            ));
        }
        let mut row =
            div()
                .flex()
                .flex_col()
                .child(label(title, TypeRole::Body, Ink::TextMuted, a));
        if let Some(reason) = &choice.reason {
            row = row.child(label(reason, TypeRole::Meta, Ink::TextFaint, a));
        }
        row
    }

    fn choices(&self, plan: &PlanView, a: Appearance, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(SPACE[0]))
            .child(label(
                t::cluster::add_choices_label(),
                TypeRole::Label,
                Ink::TextFaint,
                a,
            ))
            .child(self.choice_row(
                "add-can-be-master",
                t::cluster::add_master_label(),
                &plan.can_be_master,
                a,
                cx,
                Self::add_set_master,
            ))
            .child(self.choice_row(
                "add-acting",
                t::cluster::add_acting_label(),
                &plan.acting,
                a,
                cx,
                Self::add_set_acting,
            ))
    }
}
