//! The notch's answering cards over GPUI (part U2): "Claude asks" with a tab per question, options
//! picked by click (toggled for multi select), a text box for something else and Submit; and
//! "Permission needed" with Allow, Always allow and Deny. Both send through the daemon
//! (`session.answer`, `session.permission`) and are disabled, with the reason on hover, while
//! acting isn't on. Their drawn heights are the model's `card_height`.

use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_engine::transcript::snapshot::PendingQuestion;
use notchling_kit::metric::{CONTROL, OPTION_BADGE, RADIUS_MD, RADIUS_SM};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, field, hsla, option_row, KitStyled};
use notchling_kit::{Icon, Ink, TypeRole};

use super::answer::{type_key, QuestionDraft, Typed};
use super::cards::{line, text, A};
use super::requests::HookRequest;
use super::view::NotchView;
use crate::acting::{Control, Decision, Request};
use crate::mac::gate;

const HEADER: f32 = 22.0;
const GAP: f32 = 8.0;
const TAB_ROW: f32 = 24.0;
/// The kit's option row: 10 padding around the title over its detail, or the 22 badge.
const OPTION_PADDING: f32 = 20.0;
/// Roughly how many characters of a 13 pt line fit across a card in the panel width.
const CHARS_PER_LINE: usize = 52;
const MONO_CHARS_PER_LINE: usize = 48;
/// The permission summary shows at most this many lines (`lineLimit(10)`).
const SUMMARY_LINES: usize = 10;

fn wrapped_lines(chars: usize, per_line: usize) -> f32 {
    (chars / per_line + 1) as f32
}

/// The kit's option row always draws its detail line, empty or not.
fn option_height() -> f32 {
    let text = line(TypeRole::Heading) + 2.0 + line(TypeRole::Meta);
    text.max(OPTION_BADGE) + OPTION_PADDING
}

/// And of the 11 pt status line.
const META_CHARS_PER_LINE: usize = 62;

/// The line under the controls (why they're off, the send error, or "Sending…"), wrapped.
fn status_height(status: Option<&str>) -> f32 {
    match status {
        Some(t) => {
            wrapped_lines(t.chars().count(), META_CHARS_PER_LINE) * line(TypeRole::Meta) + GAP
        }
        None => 0.0,
    }
}

/// The question card's drawn height for the tab on show.
pub fn question_height(q: &PendingQuestion, tab: usize, status: Option<&str>) -> f32 {
    let Some(item) = q.items.get(tab).or_else(|| q.items.first()) else {
        return 0.0;
    };
    let tabs = if q.items.len() > 1 {
        TAB_ROW + GAP
    } else {
        0.0
    };
    let question = wrapped_lines(item.question.chars().count(), CHARS_PER_LINE).min(3.0)
        * line(TypeRole::Heading);
    let hint = if item.multi_select {
        line(TypeRole::Meta) + GAP
    } else {
        0.0
    };
    let options = (option_height() + GAP) * item.options.len() as f32;
    let submit = if QuestionDraft::needs_submit(q) {
        CONTROL + GAP
    } else {
        0.0
    };
    HEADER
        + GAP
        + tabs
        + question
        + GAP
        + hint
        + options
        + CONTROL
        + GAP
        + submit
        + status_height(status)
}

/// The permission card's drawn height.
pub fn permission_height(r: &HookRequest, status: Option<&str>) -> f32 {
    let lines = wrapped_lines(r.summary.chars().count(), MONO_CHARS_PER_LINE)
        + r.summary.matches('\n').count() as f32;
    let summary = lines.min(SUMMARY_LINES as f32) * line(TypeRole::Mono) + 16.0;
    let count = if r.can_always_allow { 3 } else { 2 };
    HEADER
        + GAP
        + line(TypeRole::Heading)
        + GAP
        + summary
        + GAP
        + (option_height() + GAP) * count as f32
        + status_height(status)
}

/// A card's header: icon and title in an ink, a step count, the session's name when it peeks.
fn header(icon: Icon, title: &str, ink: Ink, step: Option<String>, agent: Option<&str>) -> Div {
    div()
        .h(px(HEADER))
        .flex()
        .items_center()
        .gap(px(7.0))
        .child(text(icon.glyph(), TypeRole::Heading, ink))
        .child(text(title.to_owned(), TypeRole::Heading, ink))
        .children(step.map(|s| text(s, TypeRole::Meta, Ink::TextMuted)))
        .child(div().flex_1())
        .children(
            agent.map(|name| text(name.to_owned(), TypeRole::Meta, Ink::TextMuted).truncate()),
        )
}

impl NotchView {
    /// What the line under a card says, if anything.
    pub(super) fn card_status(&self, gate: Control) -> Option<Div> {
        let ink = if self.send_error.is_some() {
            Ink::Needs
        } else {
            Ink::TextMuted
        };
        self.status_text(gate)
            .map(|t| text(t.to_owned(), TypeRole::Meta, ink))
    }

    /// The status line's words, if it shows.
    pub(super) fn status_text(&self, gate: Control) -> Option<&str> {
        if let Some(error) = &self.send_error {
            return Some(error);
        }
        if self.sending {
            return Some("Sending…");
        }
        gate.tooltip
    }

    /// Sends an answer or a decision for a session; on success its card hides.
    fn send_for(&mut self, id: String, request: Request, cx: &mut Context<Self>) {
        if self.sending || !self.gate().enabled {
            return;
        }
        self.sending = true;
        self.send_error = None;
        let actions = self.actions.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&request) })
                .await;
            this.update(cx, |view, cx| {
                view.sending = false;
                match result {
                    Ok(_) => {
                        view.model.mark_answered(&id);
                        view.fold_peek();
                    }
                    Err(message) => view.send_error = Some(message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// "Claude asks": tabs across the questions, the one on show with its options, a text box and
    /// Submit.
    pub(super) fn question_card(
        &self,
        id: &str,
        q: &PendingQuestion,
        agent: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Div {
        let gate = self.gate();
        let tab = self.draft.tab;
        let Some(item) = q.items.get(tab) else {
            return div();
        };
        let step = (q.items.len() > 1).then(|| format!("{} of {}", tab + 1, q.items.len()));
        let tabs = (q.items.len() > 1).then(|| self.question_tabs(q, cx));
        let options = item
            .options
            .iter()
            .enumerate()
            .map(|(i, title)| {
                let detail = item.details.get(i).map(String::as_str).unwrap_or("");
                let picked = self.draft.is_picked(tab, i + 1);
                let row = option_row(
                    SharedString::from(format!("option-{tab}-{i}")),
                    i + 1,
                    title,
                    detail,
                    picked,
                    gate::state(gate),
                    A,
                )
                .line_height(px(line(TypeRole::Heading)));
                let (session, question) = (id.to_owned(), q.clone());
                gate::tip(row, gate, A).when(gate.enabled, |row| {
                    row.on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(answers) = this.draft.choose(&question, i + 1) {
                            this.send_for(
                                session.clone(),
                                Request::answer_for(&session, &question, &answers),
                                cx,
                            );
                        }
                        cx.notify();
                    }))
                })
            })
            .collect::<Vec<_>>();
        let hint = item
            .multi_select
            .then(|| text("Pick any that apply", TypeRole::Meta, Ink::TextMuted));
        let submit = QuestionDraft::needs_submit(q).then(|| self.submit_row(id, q, cx));
        div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(header(
                Icon::Question,
                "Claude asks",
                Ink::Working,
                step,
                agent,
            ))
            .children(tabs)
            .child(text(item.question.clone(), TypeRole::Heading, Ink::Text).line_clamp(3))
            .children(hint)
            .children(options)
            .child(self.answer_box(id, q, cx))
            .children(submit)
            .children(self.card_status(gate))
    }

    /// One pill per question; a tick on the answered ones. Moving between tabs answers nothing,
    /// so the tabs work whatever acting is.
    fn question_tabs(&self, q: &PendingQuestion, cx: &mut Context<Self>) -> Div {
        div()
            .h(px(TAB_ROW))
            .flex()
            .items_center()
            .gap(px(4.0))
            .children(self.draft.tabs(q).into_iter().enumerate().map(|(i, tab)| {
                let title = if tab.answered {
                    format!("{} {}", Icon::Check.glyph(), tab.title)
                } else {
                    tab.title
                };
                let ink = if tab.selected {
                    Ink::Text
                } else if tab.answered {
                    Ink::Done
                } else {
                    Ink::TextMuted
                };
                div()
                    .id(SharedString::from(format!("question-tab-{i}")))
                    .h(px(TAB_ROW))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .cursor_pointer()
                    .when(tab.selected, |t| t.fill(Ink::Raised, A))
                    .child(text(title, TypeRole::Label, ink).truncate())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.draft.select_tab(i);
                        cx.notify();
                    }))
            }))
    }

    /// "Type something else…": a one-line box; Return takes it as this question's answer.
    fn answer_box(&self, id: &str, q: &PendingQuestion, cx: &mut Context<Self>) -> Stateful<Div> {
        let gate = self.gate();
        let tab = self.draft.tab;
        let typed = self.draft.typed(tab).to_owned();
        let shown = match self.draft.text_answer(tab) {
            Some(answer) if typed.is_empty() => answer.to_owned(),
            _ => typed,
        };
        let focused = self.focused;
        let (session, question) = (id.to_owned(), q.clone());
        let element = div()
            .id(SharedString::from(format!("answer-box-{tab}")))
            .child(field(&shown, "Type something else…", focused, A).w_full())
            .when(!gate.enabled, |b| b.opacity(0.4));
        gate::tip(element, gate, A).when(gate.enabled, |b| {
            b.track_focus(&self.focus)
                .cursor_text()
                .on_click(cx.listener(|this, _, window, cx| {
                    window.focus(&this.focus);
                    cx.notify();
                }))
                .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                    let k = &event.keystroke;
                    let modified = k.modifiers.platform || k.modifiers.control;
                    let Some(typed) = this.draft.typed_mut() else {
                        return;
                    };
                    match type_key(typed, &k.key, k.key_char.as_deref(), modified) {
                        Typed::Submit => {
                            if let Some(answers) = this.draft.submit_text(&question) {
                                let request = Request::answer_for(&session, &question, &answers);
                                this.send_for(session.clone(), request, cx);
                            }
                        }
                        Typed::Changed => {}
                        Typed::Ignored => return,
                    }
                    cx.stop_propagation();
                    cx.notify();
                }))
        })
    }

    /// Next (multi select) and Submit, live once every question has an answer.
    fn submit_row(&self, id: &str, q: &PendingQuestion, cx: &mut Context<Self>) -> Div {
        let gate = self.gate();
        let ready = Control {
            enabled: gate.enabled && self.draft.complete() && !self.sending,
            tooltip: gate.tooltip,
        };
        let multi = q.items.get(self.draft.tab).is_some_and(|i| i.multi_select);
        let next = (multi && q.items.len() > 1).then(|| {
            button(
                "question-next",
                "Next",
                ButtonKind::Secondary,
                PartState::Rest,
                A,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.draft.next();
                cx.notify();
            }))
        });
        let (session, question) = (id.to_owned(), q.clone());
        let submit = button(
            "question-submit",
            "Submit",
            ButtonKind::Primary,
            gate::state(ready),
            A,
        );
        let submit = gate::tip(submit, ready, A).when(ready.enabled, |b| {
            b.on_click(cx.listener(move |this, _, _, cx| {
                if let Some(answers) = this.draft.answers(&question) {
                    this.send_for(
                        session.clone(),
                        Request::answer_for(&session, &question, &answers),
                        cx,
                    );
                }
            }))
        });
        div()
            .h(px(CONTROL))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.0))
            .children(next)
            .child(submit)
    }

    /// "Permission needed": the tool and what it would run, then Allow, Always allow and Deny.
    pub(super) fn permission_card(
        &self,
        id: &str,
        r: &HookRequest,
        agent: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Div {
        let gate = self.gate();
        let mut choices = vec![(Decision::Allow, "Allow", "This time only")];
        if r.can_always_allow {
            choices.push((
                Decision::Always,
                "Always allow this",
                "Don't ask again for this",
            ));
        }
        choices.push((Decision::Deny, "Deny", "Claude tries something else"));
        let options = choices
            .into_iter()
            .enumerate()
            .map(|(i, (decision, title, detail))| {
                let row = option_row(
                    SharedString::from(format!("permission-{i}")),
                    i + 1,
                    title,
                    detail,
                    false,
                    gate::state(gate),
                    A,
                )
                .line_height(px(line(TypeRole::Heading)));
                // The card hides by row; the decision goes to the prompt by its own id.
                let (session, prompt) = (id.to_owned(), r.id.clone());
                gate::tip(row, gate, A).when(gate.enabled, |row| {
                    row.on_click(cx.listener(move |this, _, _, cx| {
                        let request = Request::permission(&prompt, decision);
                        this.send_for(session.clone(), request, cx);
                    }))
                })
            });
        div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(header(
                Icon::Permission,
                "Permission needed",
                Ink::AccentText,
                None,
                agent,
            ))
            .child(text(
                format!("Claude wants to use {}", r.tool_name),
                TypeRole::Heading,
                Ink::Text,
            ))
            .child(
                text(r.summary.clone(), TypeRole::Mono, Ink::Text)
                    .line_clamp(SUMMARY_LINES)
                    .p(px(8.0))
                    .rounded(px(RADIUS_MD))
                    .fill(Ink::Raised, A),
            )
            .children(options)
            .children(self.card_status(gate))
    }
}

/// A small rounded mark, for the pet's badge.
pub fn badge(count: usize) -> Div {
    div()
        .min_w(px(15.0))
        .h(px(15.0))
        .px(px(4.0))
        .rounded(px(RADIUS_SM * 2.0))
        .bg(hsla(Ink::Needs.color(A)))
        .flex()
        .items_center()
        .justify_center()
        .kit_type(TypeRole::Meta)
        .text_size(px(10.0))
        .ink(Ink::OnAccent, A)
        .child(count.to_string())
}
