//! The Token Cost page over GPUI (macOS): the account filter, three tiles, the range, the day
//! bars and the breakdowns, drawn from the pure [`CostView`].

use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_kit::metric::{RADIUS_MD, SIDE_PADDING, SPINNER_SMALL as SMALL};
use notchling_kit::ui::{card, hsla, label, spinner as kit_spinner, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::cost::{Breakdown, CostView, Range, Tile, READING};
use super::view_rows::{segmented, Handler};
use crate::mac::gate;

fn tile(t: &Tile, a: Appearance) -> Div {
    card(a)
        .flex_1()
        .p(px(12.0))
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(label(t.title, TypeRole::Meta, Ink::TextMuted, a))
        .child(label(&t.value, TypeRole::Title, Ink::Text, a))
}

fn day_bars(view: &CostView, a: Appearance) -> Div {
    let mut bars = div().flex().items_end().gap(px(3.0)).h(px(72.0));
    for (i, bar) in view.bars.iter().enumerate() {
        let height = (bar.fraction * 60.0).max(2.0) as f32;
        let tip: SharedString = bar.tooltip.clone().into();
        let column = div()
            .id(SharedString::from(format!("day-{i}")))
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_end()
            .gap(px(3.0))
            .child(
                div()
                    .w_full()
                    .h(px(height))
                    .rounded(px(2.0))
                    .fill(if bar.last { Ink::Accent } else { Ink::Raised }, a),
            )
            .child(label(bar.letter, TypeRole::Meta, Ink::TextFaint, a));
        bars = bars.child(gate::tip_text(column, tip, a));
    }
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(label(view.bars_title, TypeRole::Section, Ink::TextFaint, a))
        .child(bars)
}

fn breakdown(b: &Breakdown, a: Appearance) -> Div {
    let mut rows = card(a).flex().flex_col();
    for (i, row) in b.rows.iter().enumerate() {
        rows = rows.child(
            div()
                .px(px(12.0))
                .py(px(7.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .when(i > 0, |r| {
                    r.border_t(px(1.0))
                        .border_color(hsla(Ink::Hairline.color(a)))
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            label(&row.name, TypeRole::Body, Ink::Text, a)
                                .flex_1()
                                .elide(),
                        )
                        .child(label(&row.value, TypeRole::Label, Ink::Text, a)),
                )
                .children(
                    row.detail
                        .as_deref()
                        .map(|d| label(d, TypeRole::Meta, Ink::TextFaint, a)),
                ),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(label(&b.title, TypeRole::Section, Ink::TextFaint, a))
        .child(rows)
}

/// The page. `on_range` and `on_account` take the picked range and account id.
pub fn cost_page<V: 'static>(
    view: &CostView,
    a: Appearance,
    cx: &mut Context<V>,
    on_range: Handler<V, Range>,
    on_account: Handler<V, String>,
) -> Stateful<Div> {
    let ranges: Vec<(String, String)> = Range::ALL
        .iter()
        .map(|r| (r.wire().to_owned(), r.title().to_owned()))
        .collect();
    let pick_range: Handler<V, String> = std::rc::Rc::new(move |this, key: String, cx| {
        if let Some(range) = Range::ALL.iter().find(|r| r.wire() == key) {
            on_range(this, *range, cx);
        }
    });
    let mut page = div()
        .id("cost-page")
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .p(px(SIDE_PADDING))
        .flex()
        .flex_col()
        .gap(px(18.0));
    if view.accounts.len() > 1 {
        page = page.child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(label("Account", TypeRole::Meta, Ink::TextMuted, a))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .child(segmented(
                            "cost-account",
                            &view.accounts,
                            &view.account,
                            a,
                            cx,
                            on_account,
                        )),
                ),
        );
    }
    page = page
        .child(
            div()
                .flex()
                .gap(px(10.0))
                .children(view.tiles.iter().map(|t| tile(t, a))),
        )
        .child(segmented(
            "cost-range",
            &ranges,
            view.range.wire(),
            a,
            cx,
            pick_range,
        ));
    if view.reading {
        page = page.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(kit_spinner(
                    "working-view_cost.rs-1",
                    SMALL,
                    Ink::Working,
                    a,
                ))
                .child(label(READING, TypeRole::Body, Ink::TextMuted, a)),
        );
    }
    if !view.bars.is_empty() {
        page = page.child(day_bars(view, a));
    }
    for section in &view.sections {
        page = page.child(breakdown(section, a));
    }
    page.child(
        div()
            .p(px(12.0))
            .rounded(px(RADIUS_MD))
            .fill(Ink::Card, a)
            .child(label(view.footer, TypeRole::Meta, Ink::TextFaint, a)),
    )
}
