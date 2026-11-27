use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_kit::metric::RADIUS_SM;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, label, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::semantic::{indexing_text, lines, pause_button, Line, SemanticModel};
use super::settings::Row;
use super::settings_view::SettingsView;

pub fn block(
    row: &Row,
    model: &SemanticModel,
    i: usize,
    a: Appearance,
    cx: &mut Context<SettingsView>,
) -> Div {
    let out = div()
        .debug_selector(|| "semantic-block".to_owned())
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(label(
            t::settings::semantic_search_about(),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ));
    if !model.on {
        return out;
    }
    let all = lines(model);
    out.child(
        div().flex().flex_col().children(
            all.iter()
                .enumerate()
                .map(|(k, line)| model_line(row, line, i, k, a, cx)),
        ),
    )
    .child(label(
        t::settings::model_gemma(),
        TypeRole::Meta,
        Ink::TextFaint,
        a,
    ))
    .child(label(
        t::settings::model_hosts(),
        TypeRole::Meta,
        Ink::TextFaint,
        a,
    ))
    .child(pause_line(row, model, i, a, cx))
}

fn pause_line(
    row: &Row,
    model: &SemanticModel,
    i: usize,
    a: Appearance,
    cx: &mut Context<SettingsView>,
) -> Div {
    let (option, title) = pause_button(model);
    let r = row.clone();
    div()
        .min_h(px(28.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            label(
                &indexing_text(model).unwrap_or_default(),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            )
            .flex_1(),
        )
        .child(
            button(
                SharedString::from(format!("setting-{i}-semantic-{option}")),
                title,
                ButtonKind::Secondary,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.semantic_act(&r, option, cx))),
        )
}

fn model_line(
    row: &Row,
    line: &Line,
    i: usize,
    k: usize,
    a: Appearance,
    cx: &mut Context<SettingsView>,
) -> Div {
    let (verb, title) = line.button;
    let option = format!("{verb}:{}", line.id);
    let r = row.clone();
    let heading = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(label(line.title, TypeRole::Body, Ink::Text, a))
        .children(line.recommended.then(|| {
            label(
                t::settings::model_recommended(),
                TypeRole::Meta,
                Ink::Accent,
                a,
            )
        }));
    let text = div()
        .debug_selector(|| format!("semantic-text-{k}"))
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(heading)
        .child(label(line.about, TypeRole::Meta, Ink::TextFaint, a))
        .children(
            line.status
                .as_deref()
                .map(|s| label(s, TypeRole::Meta, Ink::TextMuted, a)),
        )
        .children(line.fraction.map(|f| bar(f, a)));
    div()
        .debug_selector(|| format!("semantic-row-{k}"))
        .py(px(6.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .when(k > 0, |r| {
            r.border_t(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
        })
        .child(text)
        .child(
            button(
                SharedString::from(format!("setting-{i}-semantic-{k}-{verb}")),
                title,
                ButtonKind::Secondary,
                PartState::Rest,
                a,
            )
            .debug_selector(|| format!("semantic-button-{k}"))
            .on_click(cx.listener(move |this, _, _, cx| this.semantic_act(&r, &option, cx))),
        )
}

fn bar(fraction: f32, a: Appearance) -> Div {
    div()
        .h(px(4.0))
        .w_full()
        .rounded(px(RADIUS_SM))
        .fill(Ink::Raised, a)
        .child(
            div()
                .h_full()
                .w(gpui::relative(fraction))
                .rounded(px(RADIUS_SM))
                .fill(Ink::Accent, a),
        )
}

#[cfg(test)]
#[path = "semantic_layout_tests.rs"]
mod layout_tests;
