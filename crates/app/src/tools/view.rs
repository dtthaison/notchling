//! The tools' parts over GPUI (macOS), shared by Chat and the notch: the Antigravity permission
//! card with Antigravity's own options, the Codex model picker and the Antigravity options
//! picker as pop-up cards, and the `--render` views. Each takes the view's own click handler, so
//! the same card answers through `NotchView` in the notch and `ChatView` in Chat. Kit tokens only.

use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyView, App, Context, Div, Render, SharedString, Window};
use notchling_engine::tools::agy::ConfigOption;
use notchling_engine::transcript::model::DiffKind;
use notchling_kit::metric::{RADIUS_MD, RADIUS_SM};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, hsla, icon, label, option_row, window_appearance, KitStyled,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::agy_options::NOTE as AGY_NOTE;
use super::agy_permission::{AgyPermission, PREVIEW_LINES};
use super::codex_picker::{CodexPicker, DEFAULT_TITLE, NOTE as CODEX_NOTE, NO_MODELS};
use super::renders::{self, Shown};
use crate::acting::{Acting, Control};
use crate::actions::Actions;
use crate::mac::gate;
use crate::sheets::second_opinion_view::SecondOpinionView;
use crate::sheets::view::Acts;

/// The card's own title.
pub const PERMISSION_TITLE: &str = "Permission needed";

/// What an option does, under its name (Antigravity gives only the names).
pub fn option_detail(kind: &str) -> &'static str {
    match kind {
        "allow_once" => "This time only",
        "allow_always" => "Don't ask again for this",
        "reject_once" => "Antigravity tries something else",
        "reject_always" => "Never for this",
        _ => "",
    }
}

/// One line of the preview: the diff's sign and ink, or plain mono text.
fn preview_line(text: &str, kind: DiffKind, a: Appearance) -> Div {
    let (sign, ink) = match kind {
        DiffKind::Add => ("+ ", Ink::Done),
        DiffKind::Remove => ("− ", Ink::Expired),
        DiffKind::Context | DiffKind::Hunk => ("", Ink::Text),
    };
    label(&format!("{sign}{text}"), TypeRole::Mono, ink, a)
        .w_full()
        .line_clamp(2)
}

/// "Permission needed": what Antigravity would do (the edit's diff, else the command) and its
/// own options, each answering with its id through `answer`. Disabled, with the reason on hover,
/// while `gate` is off. The caller adds its status line.
pub fn agy_card<V: 'static>(
    agy: &AgyPermission,
    agent: Option<&str>,
    gate: Control,
    a: Appearance,
    cx: &mut Context<V>,
    answer: impl Fn(&mut V, &str, &mut Context<V>) + Clone + 'static,
) -> Div {
    let header = div()
        .h(px(22.0))
        .flex()
        .items_center()
        .gap(px(7.0))
        .child(icon(
            Icon::Permission,
            TypeRole::Heading.spec().size,
            Ink::AccentText,
            a,
        ))
        .child(label(
            PERMISSION_TITLE,
            TypeRole::Heading,
            Ink::AccentText,
            a,
        ))
        .child(div().flex_1())
        .children(agent.map(|name| label(name, TypeRole::Meta, Ink::TextMuted, a).elide()));
    let preview = div()
        .w_full()
        .p(px(8.0))
        .rounded(px(RADIUS_MD))
        .fill(Ink::Raised, a)
        .flex()
        .flex_col()
        .gap(px(1.0))
        .children(
            agy.preview(PREVIEW_LINES)
                .into_iter()
                .map(|(text, kind)| preview_line(&text, kind, a)),
        );
    let options = agy
        .ordered_options()
        .into_iter()
        .enumerate()
        .map(|(i, option)| {
            let row = option_row(
                SharedString::from(format!("agy-option-{i}")),
                i + 1,
                &option.name,
                option_detail(&option.kind),
                false,
                gate::state(gate),
                a,
            );
            let id = option.id.clone();
            let answer = answer.clone();
            gate::tip(row, gate, a).when(gate.enabled, |row| {
                row.on_click(cx.listener(move |this, _, _, cx| answer(this, &id, cx)))
            })
        })
        .collect::<Vec<_>>();
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(header)
        .child(
            label(
                &format!("Antigravity wants to {}", agy.title),
                TypeRole::Heading,
                Ink::Text,
                a,
            )
            .line_clamp(2),
        )
        .child(preview)
        .children(options)
}

/// What the Codex picker was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerAction {
    Model(String),
    Effort(String, String),
    Reset,
}

pub(crate) fn pop_up(a: Appearance) -> Div {
    card(a)
        .w(px(320.0))
        .p(px(8.0))
        .flex()
        .flex_col()
        .gap(px(4.0))
}

/// A clickable row of a pop-up: the title in the small Meta type, a faint detail under it, and a
/// check at the right end when selected. Tight padding, as a compact menu.
pub(crate) fn pick_row(
    id: impl Into<SharedString>,
    title: &str,
    detail: &str,
    selected: bool,
    a: Appearance,
) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .w_full()
        .px(px(8.0))
        .py(px(3.0))
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_pointer()
        .hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .child(label(title, TypeRole::Meta, Ink::Text, a).elide())
                .when(!detail.is_empty(), |d| {
                    d.child(label(detail, TypeRole::Meta, Ink::TextFaint, a).line_clamp(2))
                }),
        )
        .child(div().w(px(12.0)).flex_shrink_0().children(
            selected.then(|| icon(Icon::Check, TypeRole::Meta.spec().size, Ink::AccentText, a)),
        ))
}

/// A small pill for one choice in a row of them.
fn pill(
    id: impl Into<SharedString>,
    title: &str,
    selected: bool,
    a: Appearance,
) -> gpui::Stateful<Div> {
    let kind = if selected {
        ButtonKind::Secondary
    } else {
        ButtonKind::Ghost
    };
    // 8 pt a side (the 4 pt grid) instead of the button's 12, so a row of four efforts fits
    button(id.into(), title, kind, PartState::Rest, a).px(px(8.0))
}

/// The Codex model picker (`CodexModelPicker`): Codex's models, each with its efforts, and
/// Codex Default. Picks go to `act`; nothing is sent until the next turn.
pub fn codex_picker_card<V: 'static>(
    picker: &CodexPicker,
    a: Appearance,
    cx: &mut Context<V>,
    act: impl Fn(&mut V, PickerAction, &mut Context<V>) + Clone + 'static,
) -> Div {
    let mut pop = pop_up(a).child(label("Model", TypeRole::Section, Ink::TextFaint, a).px(px(8.0)));
    let rows = picker.rows();
    if rows.is_empty() {
        pop = pop.child(label(NO_MODELS, TypeRole::Meta, Ink::TextMuted, a).px(px(8.0)));
    }
    for (i, row) in rows.iter().enumerate() {
        let slug = row.slug.clone();
        let on_model = act.clone();
        pop = pop.child(
            pick_row(
                format!("model-{i}"),
                &row.name,
                &row.summary,
                row.selected,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                on_model(this, PickerAction::Model(slug.clone()), cx)
            })),
        );
        if !row.efforts.is_empty() {
            pop = pop.child(
                div()
                    .pl(px(30.0))
                    .pb(px(4.0))
                    .flex()
                    .flex_wrap()
                    .gap(px(4.0))
                    .children(row.efforts.iter().enumerate().map(|(k, e)| {
                        let (slug, effort) = (row.slug.clone(), e.effort.clone());
                        let on_effort = act.clone();
                        pill(format!("effort-{i}-{k}"), &e.title, e.selected, a).on_click(
                            cx.listener(move |this, _, _, cx| {
                                on_effort(
                                    this,
                                    PickerAction::Effort(slug.clone(), effort.clone()),
                                    cx,
                                )
                            }),
                        )
                    })),
            );
        }
    }
    let on_reset = act;
    pop.child(div().h(px(1.0)).w_full().bg(hsla(Ink::Hairline.color(a))))
        .child(
            pick_row("model-default", DEFAULT_TITLE, "", picker.is_default(), a).on_click(
                cx.listener(move |this, _, _, cx| on_reset(this, PickerAction::Reset, cx)),
            ),
        )
        .child(label(CODEX_NOTE, TypeRole::Meta, Ink::TextFaint, a).px(px(8.0)))
}

/// The Antigravity options picker (`ACPOptionsPicker`): each option the session offers with its
/// values; a pick calls `set(option id, value)`, which sends `agy.set_option`.
pub fn agy_options_card<V: 'static>(
    options: &[ConfigOption],
    gate: Control,
    a: Appearance,
    cx: &mut Context<V>,
    set: impl Fn(&mut V, &str, &str, &mut Context<V>) + Clone + 'static,
) -> Div {
    let mut pop = pop_up(a);
    for (i, option) in options.iter().enumerate() {
        pop = pop
            .child(label(&option.name, TypeRole::Section, Ink::TextFaint, a).px(px(8.0)))
            .child(
                div()
                    .px(px(8.0))
                    .pb(px(4.0))
                    .flex()
                    .flex_wrap()
                    .gap(px(4.0))
                    .children(option.values.iter().enumerate().map(|(k, v)| {
                        let (id, value) = (option.id.clone(), v.value.clone());
                        let set = set.clone();
                        let selected = v.value == option.current;
                        let pill = pill(format!("agy-option-{i}-{k}"), &v.name, selected, a);
                        gate::tip(pill, gate, a).when(gate.enabled, |p| {
                            p.on_click(
                                cx.listener(move |this, _, _, cx| set(this, &id, &value, cx)),
                            )
                        })
                    })),
            );
    }
    pop.children(
        gate.tooltip
            .map(|why| label(why, TypeRole::Meta, Ink::TextMuted, a).px(px(8.0))),
    )
    .child(label(AGY_NOTE, TypeRole::Meta, Ink::TextFaint, a).px(px(8.0)))
}

/// `--render codex-model-picker`: the picker alone, over the fixture's models cache. A click
/// changes only what is drawn.
pub struct CodexPickerRender {
    pub picker: CodexPicker,
}

impl Render for CodexPickerRender {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        div()
            .size_full()
            .p(px(16.0))
            .fill(Ink::Window, a)
            .child(codex_picker_card(
                &self.picker,
                a,
                cx,
                |this, action, cx| {
                    match action {
                        PickerAction::Model(slug) => this.picker.choose_model(&slug),
                        PickerAction::Effort(slug, effort) => {
                            this.picker.choose_effort(&slug, &effort)
                        }
                        PickerAction::Reset => this.picker.reset(),
                    }
                    cx.notify();
                },
            ))
    }
}

/// `--render agy-permission-card|codex-model-picker|second-opinion-sheet`, over fixtures; nothing
/// is ever sent.
pub fn build_render(name: &str, cx: &mut App) -> Option<AnyView> {
    Some(match renders::shown(name)? {
        Shown::AgyPermission => {
            crate::notch::backdrop::build_scene(renders::agy_permission_scene(), cx)
        }
        Shown::CodexPicker => cx
            .new(|_| CodexPickerRender {
                picker: renders::codex_picker(),
            })
            .into(),
        Shown::SecondOpinion => {
            let acts = Acts {
                actions: Arc::new(Actions::fixed(Acting::Off)),
                sessions: None,
            };
            cx.new(|cx| SecondOpinionView::new(renders::second_opinion(), acts, None, cx))
                .into()
        }
    })
}
