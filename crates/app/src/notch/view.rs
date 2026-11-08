//! The notch over GPUI (`NotchView`): the black shape at the top centre of its canvas, growing out
//! of the camera housing, and what it holds in each state. Read-only: rows open a conversation,
//! tabs switch, Back and Close move between states; nothing replies or answers.

use std::sync::Arc;

use gpui::{
    div, prelude::*, px, relative, Context, Div, FocusHandle, Render, ScrollHandle, SharedString,
    Stateful, Window,
};
use notchling_engine::sessions::{Agent, AgentGroup};
use notchling_kit::metric::RADIUS_MD;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, status_pill, KitStyled};
use notchling_kit::{Color, Ink, TypeRole};
use objc2::rc::Retained;
use objc2_app_kit::NSWindow;

use super::agy_card::agy_permission_height;
use super::answer::QuestionDraft;
use super::answering::{permission_height, question_height};
use super::cards::{bubble, needs_box, tag, text, A};
use super::layout::{Geometry, NotchState, FLARE, TAB_BAR};
use super::model::{
    ago, empty_text, last_paragraph, one_line, short_host, status, tab_title, Card, NotchModel,
    TABS,
};
use super::perch::{AxFrame, Perch, PetMode};
use super::pet::Walk;
use super::source::Source;
use crate::acting::{Acting, Control};
use crate::actions::Actions;
use crate::session_source::{Link, SessionSource};

/// The notch's root view: a transparent canvas with the shape at its top centre.
pub struct NotchView {
    pub model: NotchModel,
    /// A render draws a stand-in desktop and menu bar behind the shape; the live canvas is clear.
    backdrop: bool,
    /// The live panel's: the window, the display's geometry and the data being read.
    pub(super) ns_window: Option<Retained<NSWindow>>,
    pub(super) geometry: Option<Geometry>,
    pub(super) source: Option<Source>,
    /// Where the rows come from first: the daemon, else `source` reads the files.
    pub(super) sessions: Option<Arc<SessionSource>>,
    /// The conversation scrolls to its end when it opens and when a line arrives.
    scroll: ScrollHandle,
    scrolled_for: Option<(String, usize)>,
    /// Where the actions go, and what is known about acting.
    pub(super) actions: Arc<Actions>,
    /// The answers being put together for the question on show.
    pub(super) draft: QuestionDraft,
    pub(super) sending: bool,
    pub(super) send_error: Option<String>,
    /// The answer box's focus, and whether it had it at the last draw.
    pub(super) focus: FocusHandle,
    pub(super) focused: bool,
    /// The pet: its place, the setting, and what a render forces.
    pub(super) walk: Walk,
    pub(super) show_pet: bool,
    pub(super) forced_pet: Option<PetMode>,
    pub(super) perch: Option<Perch>,
    /// A render's stand-in for the frontmost window, in the canvas (y down).
    pub(super) scene_window: Option<AxFrame>,
}

impl NotchView {
    pub fn new(
        model: NotchModel,
        backdrop: bool,
        actions: Arc<Actions>,
        cx: &mut Context<Self>,
    ) -> NotchView {
        NotchView {
            model,
            backdrop,
            ns_window: None,
            geometry: None,
            source: None,
            sessions: None,
            scroll: ScrollHandle::new(),
            scrolled_for: None,
            actions,
            draft: QuestionDraft::default(),
            sending: false,
            send_error: None,
            focus: cx.focus_handle(),
            focused: false,
            walk: Walk::default(),
            show_pet: true,
            forced_pet: None,
            perch: None,
            scene_window: None,
        }
    }

    /// What acting is now: the daemon must be the rows' source and have said it's on.
    pub fn acting(&self) -> Acting {
        let attached = self
            .sessions
            .as_ref()
            .is_some_and(|s| s.view().link == Link::Daemon);
        self.actions.acting(attached)
    }

    /// Every action control's gate.
    pub(super) fn gate(&self) -> Control {
        self.acting().control()
    }

    /// The card on show in a state, and its drawn height.
    fn card(&self, id: &str) -> Option<(Card, f32)> {
        let card = self.model.card(id)?;
        let gate = self.gate();
        let status = self.status_text(gate);
        let height = match &card {
            Card::Question(q) => question_height(q, self.draft.tab, status),
            Card::Permission(r) => match &r.agy {
                Some(agy) => agy_permission_height(agy, status),
                None => permission_height(r, status),
            },
        };
        Some((card, height))
    }

    /// The draft follows the question on show; another question starts a fresh one.
    pub(super) fn sync_draft(&mut self) {
        let id = match &self.model.state {
            NotchState::Peek(id) | NotchState::Detail(id) => id.clone(),
            _ => return,
        };
        if let Some(Card::Question(q)) = self.model.card(&id) {
            if !self.draft.is_for(&q) {
                self.send_error = None;
            }
            self.draft.follow(&q);
        }
    }

    /// Answered from a peek: fold it away unless the pointer is on it.
    pub(super) fn fold_peek(&mut self) {
        if matches!(self.model.state, NotchState::Peek(_)) && !self.model.hovering() {
            self.model.go(NotchState::Collapsed);
        }
    }
}

/// Pure black, like the camera housing it continues; no ink is that dark.
fn black() -> gpui::Hsla {
    hsla(Color::BLACK)
}

/// The curve where the shape meets the top of the screen: an 8 pt square less a quarter circle,
/// drawn as the ring of a circle clipped to the square.
fn flare(left: bool) -> Div {
    let f = FLARE as f32;
    let ring = div()
        .absolute()
        .size(px(4.0 * f))
        .top(px(-f))
        .rounded_full()
        .border(px(f))
        .border_color(black());
    let ring = if left {
        ring.left(px(-2.0 * f))
    } else {
        ring.left(px(-f))
    };
    div()
        .relative()
        .flex_shrink_0()
        .size(px(f))
        .overflow_hidden()
        .child(ring)
}

impl NotchView {
    fn shape(&mut self, cx: &mut Context<Self>) -> Div {
        self.sync_draft();
        let state = self.model.state.clone();
        self.model.card_height = match &state {
            NotchState::Peek(id) => self.card(id).map(|(_, h)| f64::from(h)),
            _ => None,
        };
        let size = self.model.size(&state);
        let collapsed = state == NotchState::Collapsed;
        let f = FLARE as f32;
        let (w, h) = (size.w as f32, size.h as f32);
        let radius = (if collapsed { 10.0 } else { 24.0_f32 })
            .min((h - f) / 2.0)
            .min((w - 2.0 * f) / 2.0)
            .max(0.0);
        let content = match &state {
            NotchState::Collapsed => None,
            NotchState::List => Some(self.list(cx)),
            NotchState::Peek(id) => self.peek(id, cx),
            NotchState::Detail(id) => self.detail(id, cx),
        };
        let body = div()
            .w(px(w - 2.0 * f))
            .h(px(h))
            .bg(black())
            .rounded_bl(px(radius))
            .rounded_br(px(radius))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(self.top_bar())
            .children(content.map(|c| c.flex_1().min_h(px(0.0))));
        let pet = self.pet_layer(size);
        div()
            .relative()
            .flex()
            .items_start()
            .child(flare(true))
            .child(body)
            .child(flare(false))
            .children(pet)
    }

    /// The housing's own height. Active sessions are counted at its right while the shape is open
    /// on something else; the list says it in its tabs.
    fn top_bar(&self) -> Div {
        let m = &self.model;
        let active = m.count(AgentGroup::NeedsInput) + m.count(AgentGroup::Working);
        let counted = matches!(m.state, NotchState::Peek(_) | NotchState::Detail(_)) && active > 0;
        let ink = if m.count(AgentGroup::NeedsInput) > 0 {
            Ink::Needs
        } else {
            Ink::Text
        };
        div()
            .h(px(m.notch.h as f32))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .px(px(14.0))
            // Beside the camera, only while the list is open: the collapsed shape is too small.
            .child(
                div().children(
                    m.daemon_note
                        .filter(|_| m.state == NotchState::List)
                        .map(|note| text(note, TypeRole::Meta, Ink::TextFaint)),
                ),
            )
            .children(counted.then(|| text(active.to_string(), TypeRole::Label, ink)))
    }

    fn list(&self, cx: &mut Context<Self>) -> Div {
        let m = &self.model;
        let rows = m.rows(m.tab);
        let body = if rows.is_empty() {
            div()
                .h(px(40.0))
                .flex()
                .items_center()
                .justify_center()
                .child(text(empty_text(m), TypeRole::Meta, Ink::TextMuted))
                .into_any_element()
        } else {
            div()
                .id("notch-rows")
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .pl(px(6.0))
                .pr(px(12.0))
                .pt(px(2.0))
                .pb(px(6.0))
                .children(rows.into_iter().map(|a| self.row(a, cx)))
                .into_any_element()
        };
        div().flex().flex_col().child(self.tabs(cx)).child(body)
    }

    /// The three tabs under the camera, each with its count.
    fn tabs(&self, cx: &mut Context<Self>) -> Div {
        let m = &self.model;
        div()
            .h(px(TAB_BAR as f32))
            .flex_shrink_0()
            .px(px(4.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .children(TABS.map(|group| {
                let count = m.count(group);
                let selected = m.tab == group;
                let color = Ink::status(status(group));
                div()
                    .id(SharedString::from(format!("tab-{}", tab_title(group))))
                    .flex_1()
                    .h(px(26.0))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(4.0))
                    .cursor_pointer()
                    .when(selected, |t| t.fill(Ink::Raised, A))
                    .child(div().size(px(5.0)).rounded_full().fill(color, A))
                    .child(text(
                        tab_title(group),
                        TypeRole::Label,
                        if selected { Ink::Text } else { Ink::TextMuted },
                    ))
                    .child(text(
                        count.to_string(),
                        TypeRole::Label,
                        if count > 0 { color } else { Ink::TextFaint },
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.tab = group;
                        cx.notify();
                    }))
            }))
    }

    /// A row: name, status pill, host and age; then the last reply (done) or the last prompt and
    /// what it's doing.
    fn row(&self, agent: &Agent, cx: &mut Context<Self>) -> Stateful<Div> {
        let m = &self.model;
        let snapshot = m.snapshot(&agent.id);
        let group_status = status(agent.group);
        let first = div()
            .h(px(20.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                text(agent.name.clone(), TypeRole::Heading, Ink::Text)
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate(),
            )
            .child(status_pill(group_status, group_status.word(), A).flex_shrink_0())
            .child(
                text(
                    format!(
                        "{} · {}",
                        short_host(&agent.host),
                        ago(agent.since_ms, m.now_ms)
                    ),
                    TypeRole::Meta,
                    Ink::TextFaint,
                )
                .flex_shrink_0(),
            );
        let mut lines: Vec<Div> = Vec::new();
        if agent.group == AgentGroup::Completed {
            let reply = snapshot
                .and_then(|s| s.last_reply().map(str::to_owned))
                .or_else(|| agent.detail.clone());
            lines.extend(reply.map(|t| text(one_line(&t), TypeRole::Meta, Ink::TextMuted)));
        } else {
            if let Some(prompt) = snapshot.and_then(|s| s.last_prompt.as_deref()) {
                lines.push(text(
                    format!("You: {}", one_line(prompt)),
                    TypeRole::Meta,
                    Ink::TextMuted,
                ));
            }
            lines.push(text(
                one_line(&m.status_line(agent)),
                TypeRole::Meta,
                Ink::status(group_status),
            ));
        }
        let id = agent.id.clone();
        div()
            .id(SharedString::from(format!("row-{}", agent.id)))
            .h(px(m.row_height(agent) as f32))
            .flex_shrink_0()
            .px(px(8.0))
            .py(px(4.0))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .rounded(px(RADIUS_MD))
            .cursor_pointer()
            .hover(|s| s.bg(hsla(Ink::Raised.color(A))))
            .child(first)
            // One line each, cut at the edge (GPUI 0.2.2 draws no ellipsis here).
            .children(lines.into_iter().map(|l| l.truncate()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.model.open(&id);
                cx.notify();
            }))
    }

    /// A peek: the card waiting on you, or who finished and what it last said.
    fn peek(&self, id: &str, cx: &mut Context<Self>) -> Option<Div> {
        let agent = self.model.agent(id)?;
        if let Some((card, _)) = self.card(id) {
            let element = match &card {
                Card::Question(q) => self.question_card(id, q, Some(&agent.name), cx),
                Card::Permission(r) => self.permission_card(r, Some(&agent.name), cx),
            };
            return Some(
                div()
                    .px(px(12.0))
                    .pt(px(6.0))
                    .flex()
                    .flex_col()
                    .child(element),
            );
        }
        let group_status = status(agent.group);
        let open = agent.id.clone();
        let text_line = self.model.peek_text(agent).map(|t| last_paragraph(&t));
        Some(
            div().px(px(12.0)).pt(px(6.0)).flex().flex_col().child(
                div()
                    .id("peek")
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .child(
                        div()
                            .h(px(22.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(status_pill(group_status, group_status.word(), A))
                            .child(
                                text(agent.name.clone(), TypeRole::Heading, Ink::Text)
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate(),
                            )
                            .child(tag(&agent.host))
                            .child(text("Open ›", TypeRole::Meta, Ink::TextMuted)),
                    )
                    .children(text_line.map(|t| text(t, TypeRole::Body, Ink::Text).line_clamp(3)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.open(&open);
                        cx.notify();
                    })),
            ),
        )
    }

    /// One session's last lines, and its card or what it waits on.
    fn detail(&mut self, id: &str, cx: &mut Context<Self>) -> Option<Div> {
        let count = self.model.snapshot(id).map_or(0, |s| s.lines.len());
        let seen = Some((id.to_owned(), count));
        if self.scrolled_for != seen {
            self.scroll.scroll_to_bottom();
            self.scrolled_for = seen;
        }
        let m = &self.model;
        let agent = m.agent(id)?;
        let lines = m.snapshot(id).map(|s| s.lines.as_slice()).unwrap_or(&[]);
        let card = self.card(id).map(|(card, _)| match card {
            Card::Question(q) => self.question_card(id, &q, None, cx),
            Card::Permission(r) => self.permission_card(&r, None, cx),
        });
        let needs = agent
            .detail
            .as_deref()
            .filter(|d| card.is_none() && agent.group == AgentGroup::NeedsInput && !d.is_empty())
            .map(needs_box);
        let conversation = div()
            .id("notch-conversation")
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .when(lines.is_empty(), |c| {
                c.child(text("No messages yet", TypeRole::Meta, Ink::TextMuted))
            })
            .children(lines.iter().map(bubble))
            .children(needs);
        let footer = card.unwrap_or_else(|| {
            let note = if agent.label == "Exited" {
                "This session has exited."
            } else {
                "Read only here. Reply where the session runs."
            };
            text(note, TypeRole::Meta, Ink::TextMuted)
        });
        Some(
            div()
                .px(px(12.0))
                .pt(px(4.0))
                .pb(px(12.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(self.header(agent, cx))
                .child(conversation)
                .child(footer.flex_shrink_0()),
        )
    }

    fn header(&self, agent: &Agent, cx: &mut Context<Self>) -> Div {
        let group_status = status(agent.group);
        let sub = format!(
            "{} · {} · {}",
            agent.label,
            agent.host,
            ago(agent.since_ms, self.model.now_ms)
        );
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                button("notch-back", "‹", ButtonKind::Ghost, PartState::Rest, A).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.model.go(NotchState::List);
                        cx.notify();
                    }),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(text(agent.name.clone(), TypeRole::Heading, Ink::Text).truncate())
                    .child(text(sub, TypeRole::Meta, Ink::TextMuted).truncate()),
            )
            .child(status_pill(group_status, group_status.word(), A))
            .child(
                button("notch-close", "×", ButtonKind::Ghost, PartState::Rest, A).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.model.go(NotchState::Collapsed);
                        cx.notify();
                    }),
                ),
            )
    }
}

impl Render for NotchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.focused = self.focus.is_focused(window);
        let shape = self.shape(cx);
        div()
            .relative()
            .size_full()
            .line_height(relative(super::cards::LINE))
            .when(self.backdrop, |root| root.child(self.backdrop()))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(shape),
            )
    }
}
