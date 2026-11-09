//! The Second Opinion sheet over GPUI (part U8, macOS): the tool, model and effort to ask, the
//! prompt built from what is reviewed (editable), and Run. Run is disabled, with the reason on
//! hover, while acting isn't on, and while nothing runs opinions yet (`OpinionRunner`, see
//! `tools::second_opinion`).

use gpui::{
    div, prelude::*, px, Context, Div, EventEmitter, FocusHandle, Render, SharedString, Stateful,
    Window,
};
use notchling_kit::metric::{RADIUS_CARD, RADIUS_MD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, label, window_appearance, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::view::{sheet_header, Acts, Dismiss};
use crate::mac::gate;
use crate::notch::answer::{type_key, Typed};
use crate::tools::second_opinion::{tool_name, SecondOpinionSheet, Stage, NO_TOOLS};
use notchling_engine::delegate::options::Tool;
use notchling_engine::delegate::second_opinion_store::SecondOpinionStore;

pub struct SecondOpinionView {
    pub model: SecondOpinionSheet,
    acts: Acts,
    store: Option<SecondOpinionStore>,
    focus: FocusHandle,
}

impl EventEmitter<Dismiss> for SecondOpinionView {}

impl SecondOpinionView {
    pub fn new(
        model: SecondOpinionSheet,
        acts: Acts,
        store: Option<SecondOpinionStore>,
        cx: &mut Context<Self>,
    ) -> SecondOpinionView {
        SecondOpinionView {
            model,
            acts,
            store,
            focus: cx.focus_handle(),
        }
    }

    /// Run: the store records the opinion and starts `notchling delegate`; the sheet closes and
    /// the card under the message shows it running.
    fn run(&mut self, cx: &mut Context<Self>) {
        let control = self
            .model
            .run_control(self.acts.acting(), self.store.is_some());
        let Some(store) = self.store.clone().filter(|_| control.enabled) else {
            return;
        };
        let run = store.start(&self.model.request());
        self.model.started(Ok(run.id));
        cx.emit(Dismiss);
        cx.notify();
    }

    /// A row of pills, "Default" first, the picked one filled.
    #[allow(clippy::too_many_arguments)]
    fn choice_row(
        &self,
        title: &str,
        prefix: &str,
        picked: Option<&str>,
        items: &[(String, String)],
        a: Appearance,
        cx: &mut Context<Self>,
        pick: impl Fn(&mut SecondOpinionSheet, Option<&str>) + Clone + 'static,
    ) -> Div {
        let mut choices: Vec<(Option<String>, String)> = vec![(None, "Default".to_owned())];
        choices.extend(items.iter().map(|(v, n)| (Some(v.clone()), n.clone())));
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(label(title, TypeRole::Meta, Ink::TextMuted, a).w(px(48.0)))
            .child(div().flex().flex_wrap().gap(px(4.0)).children(
                choices.into_iter().enumerate().map(|(i, (value, name))| {
                    let selected = value.as_deref() == picked;
                    let kind = if selected {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    };
                    let pick = pick.clone();
                    button(
                        SharedString::from(format!("{prefix}-{i}")),
                        &name,
                        kind,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        pick(&mut this.model, value.as_deref());
                        cx.notify();
                    }))
                }),
            ))
    }

    fn tool_row(&self, a: Appearance, cx: &mut Context<Self>) -> Option<Div> {
        if self.model.tools.len() < 2 {
            return None;
        }
        Some(
            div()
                .flex()
                .p(px(2.0))
                .gap(px(2.0))
                .rounded(px(RADIUS_MD))
                .fill(Ink::Raised, a)
                .children(self.model.tools.iter().map(|tool| {
                    let tool: Tool = *tool;
                    let kind = if self.model.tool == tool {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    };
                    button(
                        SharedString::from(format!("tool-{}", tool.raw())),
                        tool_name(tool),
                        kind,
                        PartState::Rest,
                        a,
                    )
                    .flex_1()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.pick_tool(tool);
                        cx.notify();
                    }))
                })),
        )
    }

    /// The prompt, editable: typing adds at the end, Return adds a line, Backspace deletes.
    fn prompt_box(&self, a: Appearance, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let focused = self.focus.is_focused(window);
        div()
            .id("second-opinion-prompt")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p(px(10.0))
            .rounded(px(RADIUS_MD))
            .fill(Ink::Raised, a)
            .border(px(1.0))
            .border_color(hsla(
                if focused { Ink::Accent } else { Ink::Hairline }.color(a),
            ))
            .track_focus(&self.focus)
            .cursor_text()
            .child(label(&self.model.prompt, TypeRole::Mono, Ink::Text, a))
            .on_click(cx.listener(|this, _, window, cx| {
                window.focus(&this.focus);
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                let k = &event.keystroke;
                let modified = k.modifiers.platform || k.modifiers.control;
                match type_key(
                    &mut this.model.prompt,
                    &k.key,
                    k.key_char.as_deref(),
                    modified,
                ) {
                    Typed::Submit => this.model.prompt.push('\n'),
                    Typed::Changed => {}
                    Typed::Ignored => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
    }

    fn footer(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let control = self
            .model
            .run_control(self.acts.acting(), self.store.is_some());
        let failed = matches!(self.model.stage, Stage::Failed(_));
        let cancel = button(
            "cancel",
            "Cancel",
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|_, _, _, cx| cx.emit(Dismiss)));
        let run = button("run", "Run", ButtonKind::Primary, gate::state(control), a);
        let run = gate::tip(run, control, a).when(control.enabled, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.run(cx)))
        });
        div()
            .flex_shrink_0()
            .px(px(SIDE_PADDING))
            .py(px(12.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .border_t(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(
                label(
                    &self.model.footer(),
                    TypeRole::Meta,
                    if failed { Ink::Needs } else { Ink::TextFaint },
                    a,
                )
                .flex_1()
                .min_w(px(0.0))
                .line_clamp(2),
            )
            .child(cancel)
            .child(run)
    }
}

impl Render for SecondOpinionView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let close = button("close", "Close", ButtonKind::Secondary, PartState::Rest, a)
            .on_click(cx.listener(|_, _, _, cx| cx.emit(Dismiss)));
        let models: Vec<(String, String)> = self
            .model
            .models()
            .iter()
            .map(|m| (m.model.clone(), m.name.clone()))
            .collect();
        let efforts: Vec<(String, String)> = self
            .model
            .efforts()
            .into_iter()
            .map(|e| {
                let mut chars = e.chars();
                let title = match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                };
                (e, title)
            })
            .collect();
        let body = if self.model.tools.is_empty() {
            div().child(label(NO_TOOLS, TypeRole::Body, Ink::TextMuted, a))
        } else {
            div()
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .children(self.tool_row(a, cx))
                .child(self.choice_row(
                    "Model",
                    "model",
                    self.model.model.as_deref(),
                    &models,
                    a,
                    cx,
                    |m, v| m.pick_model(v),
                ))
                .child(self.choice_row(
                    "Effort",
                    "effort",
                    self.model.effort.as_deref(),
                    &efforts,
                    a,
                    cx,
                    |m, v| m.pick_effort(v),
                ))
                // The section role draws uppercase; the file name keeps its own case.
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(label("Prompt", TypeRole::Section, Ink::TextFaint, a))
                        .child(label(
                            &format!("· {}", self.model.subject_title()),
                            TypeRole::Meta,
                            Ink::TextFaint,
                            a,
                        )),
                )
                .child(self.prompt_box(a, window, cx))
        };
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .overflow_hidden()
            .fill(Ink::Window, a)
            .child(sheet_header(Icon::Check, "Second Opinion", close, a))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .p(px(SIDE_PADDING))
                    .flex()
                    .flex_col()
                    .child(body),
            )
            .child(self.footer(a, cx))
    }
}
