use std::rc::Rc;

use gpui::{div, prelude::*, px, Context, Div};
use notchling_kit::ui::{card, hsla, label};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::spend::{Column, Span};
use super::view::CostsView;
use crate::accounts::view_rows::{segmented, Handler};

fn column_card(col: &Column, first: bool, a: Appearance) -> Div {
    let mut body = div()
        .flex_1()
        .min_w(px(0.0))
        .px(px(14.0))
        .py(px(10.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .when(!first, |d| {
            d.border_l(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
        })
        .child(label(col.title, TypeRole::Section, Ink::TextFaint, a))
        .child(label(&col.range, TypeRole::Meta, Ink::TextMuted, a))
        .child(label(&col.headline, TypeRole::Title, Ink::Text, a));
    if let Some(detail) = &col.detail {
        body = body.child(label(detail, TypeRole::Body, Ink::Text, a));
    }
    if let Some(activity) = &col.activity {
        body = body.child(label(activity, TypeRole::Meta, Ink::TextMuted, a));
    }
    if let Some(unpriced) = &col.unpriced {
        body = body.child(label(unpriced, TypeRole::Meta, Ink::TextFaint, a));
    }
    body
}

impl CostsView {
    pub(super) fn spend_section(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let options: Vec<(String, String)> = Span::ALL
            .iter()
            .map(|s| (s.key().to_owned(), s.title().to_owned()))
            .collect();
        let pick: Handler<CostsView, String> = Rc::new(|this, key, cx| {
            if let Some(span) = Span::from_key(&key) {
                this.set_span(span, cx);
            }
        });
        let view = self.spend.view();
        let mut out = div().flex().flex_col().gap(px(6.0)).child(
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    label(
                        t::costs::spend_title(),
                        TypeRole::Section,
                        Ink::TextFaint,
                        a,
                    )
                    .flex_1(),
                )
                .child(segmented(
                    "costs-spend-span",
                    &options,
                    self.spend.span.key(),
                    a,
                    cx,
                    pick,
                )),
        );
        if let Some(why) = &self.spend.message {
            return out.child(label(why, TypeRole::Meta, Ink::Needs, a));
        }
        if let Some(empty) = view.empty {
            return out.child(label(empty, TypeRole::Meta, Ink::TextMuted, a));
        }
        if let Some(intro) = &view.intro {
            out = out.child(label(intro, TypeRole::Meta, Ink::TextMuted, a));
        }
        let mut cards = card(a).flex().items_start();
        for (i, col) in view.columns.iter().enumerate() {
            cards = cards.child(column_card(col, i == 0, a));
        }
        out = out.child(cards);
        for line in [&view.change, &view.note].into_iter().flatten() {
            out = out.child(label(line, TypeRole::Body, Ink::Text, a));
        }
        out.child(label(
            t::costs::spend_footer(),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ))
    }
}
