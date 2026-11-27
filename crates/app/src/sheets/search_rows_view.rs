use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_engine::search::query::category::Category;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{hsla, icon_button, label};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::search_rows::{lines, Line};
use super::settings::Row;
use super::settings_view::{switch, SettingsView};

pub fn categories_block(
    row: &Row,
    rows: &[(Category, bool)],
    i: usize,
    a: Appearance,
    cx: &mut Context<SettingsView>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .children(
            lines(rows)
                .into_iter()
                .enumerate()
                .map(|(k, line)| category_line(row, &line, i, k, a, cx)),
        )
        .child(label(
            t::settings::search_category_order(),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ))
}

fn category_line(
    row: &Row,
    line: &Line,
    i: usize,
    k: usize,
    a: Appearance,
    cx: &mut Context<SettingsView>,
) -> Div {
    let id = |part: &str| SharedString::from(format!("setting-{i}-{}-{part}", line.category.id()));
    let (toggle, up, down) = (row.clone(), row.clone(), row.clone());
    let (on_toggle, on_up, on_down) = (
        format!("toggle:{}", line.category.id()),
        format!("up:{}", line.category.id()),
        format!("down:{}", line.category.id()),
    );
    let step = |glyph: Icon, part: &str, allowed: bool| {
        let state = if allowed {
            PartState::Rest
        } else {
            PartState::Disabled
        };
        icon_button(id(part), glyph, ButtonKind::Ghost, state, a)
    };
    let up_button = step(Icon::ArrowUp, "up", !line.first);
    let down_button = step(Icon::ArrowDown, "down", !line.last);
    div()
        .min_h(px(28.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .when(k > 0, |r| {
            r.border_t(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
        })
        .child(label(line.title, TypeRole::Body, Ink::Text, a).flex_1())
        .child(if line.first {
            up_button
        } else {
            up_button.on_click(cx.listener(move |this, _, _, cx| this.apply(&up, &on_up, cx)))
        })
        .child(if line.last {
            down_button
        } else {
            down_button.on_click(cx.listener(move |this, _, _, cx| this.apply(&down, &on_down, cx)))
        })
        .child(
            switch(id("switch"), line.on, a)
                .on_click(cx.listener(move |this, _, _, cx| this.apply(&toggle, &on_toggle, cx))),
        )
}
