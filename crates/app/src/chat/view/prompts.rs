//! The question or permission a Claude Code session waits on, as a card above the reply box, the
//! composer's width: "Claude asks" (a tab per question, options picked by click, toggled for
//! multi select, a box for something else, Next and Submit) and "Permission needed" (Allow, Always
//! allow, Deny). What it shows and where it comes from is `chat::prompts`; the answers are the
//! notch's (`notch::answer::QuestionDraft`, `acting::Request::answer_for`/`permission`), sent
//! through the same daemon ops; a permission read only from a job's state goes to its terminal
//! (`chat::prompts::permission_request`). Answering here hides the card in the notch, and the other way
//! round, by the session source's one answered set. Every control follows `Acting::control`: drawn
//! disabled with the reason, and nothing is sent, while acting isn't on.

use std::collections::HashMap;

use gpui::{
    div, prelude::*, px, Context, Div, FocusHandle, KeyDownEvent, SharedString, Stateful, Window,
};
use notchling_engine::transcript::snapshot::PendingQuestion;
use notchling_kit::labels::Corner;
use notchling_kit::metric::{CONTROL, RADIUS_COMPOSER, RADIUS_MD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, field, hsla, icon, label, option_row, KitPart, KitStyled};
use notchling_kit::{sentence, Appearance, Icon, Ink, TypeRole};

use super::ChatView;
use crate::acting::{Control, Decision, Request};
use crate::chat::data::Tool;
use crate::chat::prompts::{
    is_dialog, permission_request, preview_prompt, prompt_for, question_key, screen_question,
    screen_read_key, Preview, Prompt,
};
use crate::mac::gate;
use crate::notch::answer::{type_key, QuestionDraft, Typed};
use crate::notch::requests::HookRequest;
use crate::phone::bridge::now_ms;

/// How long after a screen read that found no question the next one may go.
const SCREEN_RETRY_MS: i64 = 3_000;

/// The permission's command shows at most this many lines.
const SUMMARY_LINES: usize = 12;

/// What Chat keeps for the prompt card.
pub struct State {
    /// The prompt on show, worked out once per draw (`ChatView::sync_prompt`).
    pub shown: Option<Prompt>,
    /// The Swift hook's held prompts, read while the daemon's aren't followed.
    pub files: HashMap<String, HookRequest>,
    /// The selected session's unanswered question in its transcript.
    pub transcript: Option<PendingQuestion>,
    /// The question read off a job's screen, by its read key (`chat::prompts::screen_read_key`).
    screen: Option<(String, PendingQuestion)>,
    /// The read key last asked for, so each question is read once (an answer or a new question
    /// changes the job's summary, and with it the key).
    screen_asked: Option<String>,
    /// When a read that found nothing may be tried again (the dialog draws after the state says
    /// it waits), in ms.
    screen_retry_ms: i64,
    draft: QuestionDraft,
    sending: bool,
    error: Option<String>,
    focus: FocusHandle,
    /// A render's forced prompt (`NOTCHLING_RENDER_PROMPT`).
    pub(super) preview: Option<Preview>,
}

impl State {
    pub fn new(preview: Option<Preview>, cx: &mut Context<ChatView>) -> State {
        State {
            shown: None,
            files: HashMap::new(),
            transcript: None,
            screen: None,
            screen_asked: None,
            screen_retry_ms: 0,
            draft: QuestionDraft::default(),
            sending: false,
            error: None,
            focus: cx.focus_handle(),
            preview,
        }
    }

    /// Whether a card is up: the reply box's refusal points at it then.
    pub fn is_shown(&self) -> bool {
        self.shown.is_some()
    }
}

impl ChatView {
    /// The prompt the selected session waits on, from the sources in `chat::prompts`.
    fn prompt_now(&self) -> Option<Prompt> {
        let session = self.selected_session()?;
        if let Some(preview) = self.prompt.preview {
            return (session.tool == Tool::Claude).then(|| preview_prompt(preview));
        }
        let (prompts, answered) = self.source.held();
        let key = screen_read_key(&session.agent, session.tool);
        let screen = self
            .prompt
            .screen
            .as_ref()
            .filter(|(k, _)| key.as_ref() == Some(k))
            .map(|(_, q)| q);
        prompt_for(
            &session.agent,
            session.tool,
            prompts.as_deref(),
            &self.prompt.files,
            self.prompt.transcript.as_ref(),
            screen,
            &answered,
        )
    }

    /// Asks the daemon to read the question off the selected job's screen, once per question, when
    /// no hook holds it and the transcript doesn't have it yet. Nothing is asked while acting is
    /// off (the read moves the dialog's tabs) or in a render.
    fn read_screen_question(&mut self, cx: &mut Context<Self>) {
        if self.prompt.preview.is_some() || self.root.fixture.is_some() {
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        let Some(key) = screen_read_key(&session.agent, session.tool) else {
            return;
        };
        let row = session.agent.id.clone();
        let held = matches!(&self.prompt.shown, Some(Prompt::Question(q)) if q.tool_use_id.starts_with("hook:"));
        if held
            || self.prompt.transcript.is_some()
            || (self.prompt.screen_asked.as_ref() == Some(&key)
                && (self.prompt.screen.as_ref().is_some_and(|(k, _)| *k == key)
                    || now_ms() < self.prompt.screen_retry_ms))
            || !self.acting(cx).control().enabled
        {
            return;
        }
        self.prompt.screen_asked = Some(key.clone());
        // Never two reads at once: the next try waits for this one's answer, then a pause.
        self.prompt.screen_retry_ms = i64::MAX;
        let actions = self.acts(cx).actions;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&Request::question(&row)) })
                .await;
            this.update(cx, |view, cx| {
                match result.ok().as_ref().and_then(screen_question) {
                    Some(q) => view.prompt.screen = Some((key, q)),
                    // Nothing on screen yet, or the read failed: tried again after a pause.
                    None => view.prompt.screen_retry_ms = now_ms() + SCREEN_RETRY_MS,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Before a draw: works out the prompt on show, and keeps the answers being put together
    /// following it (another question starts afresh, and so does its error).
    pub(super) fn sync_prompt(&mut self, cx: &mut Context<Self>) {
        self.read_screen_question(cx);
        let shown = self.prompt_now();
        let key = |p: &Option<Prompt>| p.as_ref().map(Prompt::key);
        if key(&shown) != key(&self.prompt.shown) {
            self.prompt.error = None;
        }
        if let Some(Prompt::Question(q)) = &shown {
            self.prompt.draft.follow(q);
        }
        self.prompt.shown = shown;
    }

    /// The card's gate: acting, and a render's forced prompt is live.
    fn prompt_gate(&self, cx: &mut Context<Self>) -> Control {
        if self.prompt.preview.is_some() {
            return Control {
                enabled: true,
                tooltip: None,
            };
        }
        self.acting(cx).control()
    }

    /// Sends an answer or a decision; on success the prompt it answered (`key`, taken when the
    /// button was pressed) is hidden everywhere, and no other: the session may have a newer one
    /// by the time the reply comes back.
    fn answer_prompt(&mut self, key: String, request: Request, cx: &mut Context<Self>) {
        if self.prompt.sending || !self.prompt_gate(cx).enabled {
            return;
        }
        self.prompt.sending = true;
        self.prompt.error = None;
        // As a sent message does: the answer and what the session does next show at the end.
        self.follow_sent_now(cx);
        let actions = self.acts(cx).actions;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&request) })
                .await;
            this.update(cx, |view, cx| {
                view.prompt.sending = false;
                match result {
                    Ok(_) => {
                        view.source.mark_answered(&key);
                        // "Answer it above" is answered.
                        if view.tools.error.as_deref().is_some_and(is_dialog) {
                            view.tools.error = None;
                        }
                        view.reload_now(cx);
                    }
                    Err(message) => view.prompt.error = Some(message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

/// The line under the controls: the send error, "Sending…", or why they are off.
fn status(view: &ChatView, gate: Control, a: Appearance) -> Option<Div> {
    let (text, ink) = if let Some(error) = &view.prompt.error {
        (error.as_str(), Ink::Needs)
    } else if view.prompt.sending {
        ("Sending…", Ink::TextMuted)
    } else {
        (gate.tooltip?, Ink::TextMuted)
    };
    Some(label(&sentence(text), TypeRole::Meta, ink, a))
}

/// A card's header: icon and title in an ink, and a step count.
fn header(icon_: Icon, title: &str, ink: Ink, step: Option<String>, a: Appearance) -> Div {
    div()
        .h(px(22.0))
        .flex()
        .items_center()
        .gap(px(7.0))
        .child(icon(icon_, TypeRole::Heading.spec().size, ink, a))
        .child(label(title, TypeRole::Heading, ink, a))
        .children(step.map(|s| label(&s, TypeRole::Meta, Ink::TextMuted, a)))
}

/// The card's frame: the composer's width, an accent edge as the permission card has.
fn frame(prompt: &Prompt, content: Div, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    div()
        .mx(px(SIDE_PADDING))
        .mb(px(8.0))
        .p(px(14.0))
        .rounded(px(RADIUS_COMPOSER))
        .fill(Ink::Card, a)
        .border(px(1.0))
        .border_color(hsla(Ink::Accent.color(a).with_alpha(0.45)))
        .child(content)
        .part_at(prompt.part(), Corner::TopTrailing, a, cx)
}

/// The card above the reply box, when the session waits on something.
pub(super) fn card(
    view: &ChatView,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Option<Div> {
    let prompt = view.prompt.shown.clone()?;
    let row = view.selected.clone()?;
    let gate = view.prompt_gate(cx);
    let content = match &prompt {
        Prompt::Question(q) => question(view, &row, q, gate, a, window, cx),
        Prompt::Permission(r) => permission(view, r, gate, a, cx),
    };
    Some(frame(&prompt, content, a, cx))
}

/// "Claude asks": tabs across the questions, the one on show with its options, a box and Submit.
fn question(
    view: &ChatView,
    row: &str,
    q: &PendingQuestion,
    gate: Control,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Div {
    let draft = &view.prompt.draft;
    let tab = draft.tab;
    let Some(item) = q.items.get(tab) else {
        return div();
    };
    let step = (q.items.len() > 1).then(|| format!("{} of {}", tab + 1, q.items.len()));
    let tabs = (q.items.len() > 1).then(|| tabs(view, q, a, cx));
    let options = item
        .options
        .iter()
        .enumerate()
        .map(|(i, title)| {
            let detail = item.details.get(i).map(String::as_str).unwrap_or("");
            let row_el = option_row(
                SharedString::from(format!("prompt-option-{tab}-{i}")),
                i + 1,
                title,
                detail,
                draft.is_picked(tab, i + 1),
                gate::state(gate),
                a,
            );
            let (session, question) = (row.to_owned(), q.clone());
            gate::tip(row_el, gate, a).when(gate.enabled, |el| {
                el.on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(answers) = this.prompt.draft.choose(&question, i + 1) {
                        let request = Request::answer_for(&session, &question, &answers);
                        this.answer_prompt(question_key(&question), request, cx);
                    }
                    cx.notify();
                }))
            })
        })
        .collect::<Vec<_>>();
    let hint = item
        .multi_select
        .then(|| label("Pick any that apply", TypeRole::Meta, Ink::TextMuted, a));
    let submit = QuestionDraft::needs_submit(q).then(|| submit_row(view, row, q, gate, a, cx));
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(header(Icon::Question, "Claude asks", Ink::Working, step, a))
        .children(tabs)
        .child(label(&item.question, TypeRole::Heading, Ink::Text, a))
        .children(hint)
        .children(options)
        .child(answer_box(view, row, q, gate, a, window, cx))
        .children(submit)
        .children(status(view, gate, a))
}

/// One pill per question; a tick on the answered ones. Moving between tabs answers nothing, so
/// the tabs work whatever acting is.
fn tabs(view: &ChatView, q: &PendingQuestion, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    div()
        .h(px(24.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .children(
            view.prompt
                .draft
                .tabs(q)
                .into_iter()
                .enumerate()
                .map(|(i, t)| {
                    let ink = if t.selected {
                        Ink::Text
                    } else if t.answered {
                        Ink::Done
                    } else {
                        Ink::TextMuted
                    };
                    div()
                        .id(SharedString::from(format!("prompt-tab-{i}")))
                        .h(px(24.0))
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .rounded_full()
                        .cursor_pointer()
                        .when(t.selected, |p| p.fill(Ink::Raised, a))
                        .when(t.answered, |p| p.child(icon(Icon::Check, 10.0, ink, a)))
                        .child(label(&t.title, TypeRole::Label, ink, a).elide())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.prompt.draft.select_tab(i);
                            cx.notify();
                        }))
                }),
        )
}

/// "Type something else…": a one-line box; Return takes it as this question's answer.
fn answer_box(
    view: &ChatView,
    row: &str,
    q: &PendingQuestion,
    gate: Control,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    let draft = &view.prompt.draft;
    let tab = draft.tab;
    let typed = draft.typed(tab).to_owned();
    let shown = match draft.text_answer(tab) {
        Some(answer) if typed.is_empty() => answer.to_owned(),
        _ => typed,
    };
    let focused = view.prompt.focus.is_focused(window);
    let (session, question) = (row.to_owned(), q.clone());
    let element = div()
        .id(SharedString::from(format!("prompt-answer-{tab}")))
        .child(field(&shown, "Type something else…", focused, a).w_full())
        .when(!gate.enabled, |b| b.opacity(0.4));
    gate::tip(element, gate, a).when(gate.enabled, |b| {
        b.track_focus(&view.prompt.focus)
            .cursor_text()
            .on_click(cx.listener(|this, _, window, cx| {
                window.focus(&this.prompt.focus);
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let k = &event.keystroke;
                let modified = k.modifiers.platform || k.modifiers.control;
                let Some(typed) = this.prompt.draft.typed_mut() else {
                    return;
                };
                match type_key(typed, &k.key, k.key_char.as_deref(), modified) {
                    Typed::Submit => {
                        if let Some(answers) = this.prompt.draft.submit_text(&question) {
                            let request = Request::answer_for(&session, &question, &answers);
                            this.answer_prompt(question_key(&question), request, cx);
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
fn submit_row(
    view: &ChatView,
    row: &str,
    q: &PendingQuestion,
    gate: Control,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let draft = &view.prompt.draft;
    let ready = Control {
        enabled: gate.enabled && draft.complete() && !view.prompt.sending,
        tooltip: gate.tooltip,
    };
    let multi = q.items.get(draft.tab).is_some_and(|i| i.multi_select);
    let next = (multi && q.items.len() > 1).then(|| {
        button(
            "prompt-next",
            "Next",
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.prompt.draft.next();
            cx.notify();
        }))
    });
    let (session, question) = (row.to_owned(), q.clone());
    let submit = button(
        "prompt-submit",
        "Submit",
        ButtonKind::Primary,
        gate::state(ready),
        a,
    );
    let submit = gate::tip(submit, ready, a).when(ready.enabled, |b| {
        b.on_click(cx.listener(move |this, _, _, cx| {
            if let Some(answers) = this.prompt.draft.answers(&question) {
                let request = Request::answer_for(&session, &question, &answers);
                this.answer_prompt(question_key(&question), request, cx);
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
fn permission(
    view: &ChatView,
    r: &HookRequest,
    gate: Control,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
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
                SharedString::from(format!("prompt-permission-{i}")),
                i + 1,
                title,
                detail,
                false,
                gate::state(gate),
                a,
            );
            // A hook's prompt is decided by its own id; one read from the job's state is typed
            // into its terminal, checked against what this card shows. Either hides that one only.
            let (key, request) = (r.id.clone(), permission_request(r, decision));
            gate::tip(row, gate, a).when(gate.enabled, |el| {
                el.on_click(cx.listener(move |this, _, _, cx| {
                    this.answer_prompt(key.clone(), request.clone(), cx);
                }))
            })
        });
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(header(
            Icon::Permission,
            "Permission needed",
            Ink::AccentText,
            None,
            a,
        ))
        .child(label(
            &format!("Claude wants to use {}", r.tool_name),
            TypeRole::Heading,
            Ink::Text,
            a,
        ))
        .child(
            label(&r.summary, TypeRole::Mono, Ink::Text, a)
                .line_clamp(SUMMARY_LINES)
                .p(px(8.0))
                .rounded(px(RADIUS_MD))
                .fill(Ink::Raised, a),
        )
        .children(options)
        .children(status(view, gate, a))
}
