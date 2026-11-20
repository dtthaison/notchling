use std::rc::Rc;

use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_kit::metric::PILL_HEIGHT;
use notchling_kit::part::ButtonKind;
use notchling_kit::ui::{card, hsla, label, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::cache::Ttl;
use super::check::{Dot, Drift, Resolve, Row};
use super::profile::tool_title;
use super::view::CostsView;
use crate::mac::gate;
use crate::sheets::client_view::{action, Click};

pub(super) fn dot_ink(dot: Dot) -> Ink {
    match dot {
        Dot::Done => Ink::Done,
        Dot::Working => Ink::Working,
        Dot::Needs => Ink::Needs,
        Dot::Muted => Ink::TextFaint,
    }
}

pub(super) fn drift_word(state: Drift) -> &'static str {
    match state {
        Drift::InStep => t::costs::drift_in_step(),
        Drift::Outdated => t::costs::drift_outdated(),
        Drift::Edited => t::costs::drift_edited(),
        Drift::Missing => t::costs::drift_missing(),
        Drift::ChangedByTool => t::costs::drift_changed_by_tool(),
        Drift::Returned => t::costs::drift_returned(),
        Drift::Skipped => t::costs::drift_skipped(),
        Drift::Yours => t::costs::drift_yours(),
    }
}

fn resolve_word(choice: Resolve) -> &'static str {
    match choice {
        Resolve::Keep => t::costs::check_keep(),
        Resolve::Restore => t::costs::check_restore(),
    }
}

fn pill(text: &str, a: Appearance) -> Div {
    div()
        .h(px(PILL_HEIGHT))
        .px(px(8.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .rounded_full()
        .fill(Ink::Raised, a)
        .child(label(text, TypeRole::Label, Ink::TextMuted, a))
}

fn ttl_text(ttl: &Ttl) -> String {
    match ttl {
        Ttl::FiveMinutes => t::costs::ttl_five_minutes().to_owned(),
        Ttl::OneHour => t::costs::ttl_one_hour().to_owned(),
        Ttl::Other(text) => t::costs::ttl_other(text),
    }
}

impl CostsView {
    fn check_row(&self, row: &Row, first: bool, a: Appearance, cx: &mut Context<Self>) -> Div {
        let mut detail = format!("{} · {}", tool_title(row.tool), drift_word(row.state));
        if let Some(extra) = &row.detail {
            detail = t::costs::joined(&detail, extra);
        }
        let id = SharedString::from(format!("costs-dot-{}", row.id));
        let dot = gate::tip_text(
            div()
                .id(id)
                .flex_shrink_0()
                .size(px(8.0))
                .rounded_full()
                .fill(dot_ink(row.state.dot()), a),
            SharedString::from(drift_word(row.state)),
            a,
        );
        let busy = self.check.busy.as_deref() == Some(row.id.as_str());
        let enabled = self.check.can_resolve();
        let buttons = row.state.resolutions().iter().map(|choice| {
            let click: Click = {
                let (id, choice) = (row.id.clone(), *choice);
                Rc::new(cx.listener(move |this, _, _, cx| {
                    this.load_check(Some((id.clone(), choice)), cx)
                }))
            };
            let slot: &'static str = match choice {
                Resolve::Keep => "costs-check-keep",
                Resolve::Restore => "costs-check-restore",
            };
            let shown = action(
                slot,
                resolve_word(*choice),
                ButtonKind::Secondary,
                enabled,
                &click,
                a,
            );
            div()
                .id(SharedString::from(format!("{slot}-{}", row.id)))
                .child(shown)
        });
        let mut line = div()
            .px(px(14.0))
            .py(px(8.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .when(!first, |r| {
                r.border_t(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
            })
            .child(dot)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .child(label(&row.path, TypeRole::Mono, Ink::Text, a).elide())
                    .child(label(&detail, TypeRole::Meta, Ink::TextMuted, a)),
            );
        if busy {
            line = line.child(label(
                t::costs::check_working(),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ));
        } else {
            line = line.children(buttons);
        }
        line
    }

    pub(super) fn check_section(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let model = &self.check;
        let summary = if !model.loaded {
            t::costs::check_loading().to_owned()
        } else if model.message.is_some() && model.rows.is_empty() {
            t::costs::check_failed().to_owned()
        } else if model.rows.is_empty() {
            t::costs::check_empty().to_owned()
        } else if model.drifted() == 0 {
            t::costs::check_clean().to_owned()
        } else {
            t::costs::check_drift(model.drifted() as u64, model.rows.len() as u64)
        };
        let again: Click = Rc::new(cx.listener(|this, _, _, cx| this.load_check(None, cx)));
        let mut rows = card(a).flex().flex_col().child(
            div()
                .px(px(14.0))
                .py(px(8.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(label(&summary, TypeRole::Body, Ink::Text, a).flex_1())
                .child(action(
                    "costs-check-again",
                    t::costs::check_again(),
                    ButtonKind::Secondary,
                    model.busy.is_none(),
                    &again,
                    a,
                )),
        );
        for row in &model.rows {
            rows = rows.child(self.check_row(row, false, a, cx));
        }
        let mut out = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(label(
                t::costs::check_title(),
                TypeRole::Section,
                Ink::TextFaint,
                a,
            ))
            .child(rows);
        if let Some(message) = &model.message {
            out = out.child(label(message, TypeRole::Meta, Ink::Needs, a));
        } else if model.loaded && !model.on && !model.rows.is_empty() {
            out = out.child(label(
                t::costs::check_install_off(),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ));
        }
        out.child(label(
            t::costs::check_footer(),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ))
    }

    pub(super) fn cache_section(&self, a: Appearance) -> Div {
        let row = |title: &str, line: &str, note: &str, first: bool| {
            div()
                .px(px(14.0))
                .py(px(8.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .when(!first, |r| {
                    r.border_t(px(1.0))
                        .border_color(hsla(Ink::Hairline.color(a)))
                })
                .child(
                    label(title, TypeRole::Body, Ink::Text, a)
                        .w(px(110.0))
                        .flex_shrink_0(),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .child(label(line, TypeRole::Body, Ink::Text, a))
                        .child(label(note, TypeRole::Meta, Ink::TextMuted, a)),
                )
        };
        let cache = &self.cache;
        let source = if cache.set_main || cache.set_helpers {
            t::costs::cache_from_settings()
        } else {
            t::costs::cache_default()
        };
        let claude_line = t::costs::cache_claude(&ttl_text(&cache.main), &ttl_text(&cache.helpers));
        let warm = card(a).flex().flex_col().child(
            div()
                .px(px(14.0))
                .py(px(10.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .child(label(
                            t::costs::keep_warm_title(),
                            TypeRole::Body,
                            Ink::Text,
                            a,
                        ))
                        .child(label(
                            t::costs::keep_warm_note(),
                            TypeRole::Meta,
                            Ink::TextMuted,
                            a,
                        )),
                )
                .child(pill(t::costs::coming_later(), a)),
        );
        let times = card(a)
            .flex()
            .flex_col()
            .child(row(tool_title(0), &claude_line, source, true))
            .child(row(
                tool_title(1),
                t::costs::cache_codex(),
                t::costs::cache_provider_note(),
                false,
            ))
            .child(row(
                tool_title(2),
                t::costs::cache_agy(),
                t::costs::cache_provider_note(),
                false,
            ));
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(label(
                        t::costs::keep_warm_section(),
                        TypeRole::Section,
                        Ink::TextFaint,
                        a,
                    ))
                    .child(warm),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(label(
                        t::costs::cache_section_title(),
                        TypeRole::Section,
                        Ink::TextFaint,
                        a,
                    ))
                    .child(times)
                    .child(label(
                        t::costs::cache_footer(),
                        TypeRole::Meta,
                        Ink::TextFaint,
                        a,
                    )),
            )
    }
}
