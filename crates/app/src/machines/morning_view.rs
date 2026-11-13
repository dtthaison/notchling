use gpui::{div, prelude::*, px, Context, Div, SharedString, Stateful};
use notchling_engine::machines::merge::MergeOutcome;
use notchling_engine::machines::morning::{MorningItem, MorningKind};
use notchling_engine::machines::status::{NeedsYouEntry, ReadyEntry, StuckEntry};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, field, icon, label};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::view::{busy_line, mono_box, text_line, Confirming, MachinesView, CARD_PADDING};

fn state(busy: bool) -> PartState {
    if busy {
        PartState::Disabled
    } else {
        PartState::Rest
    }
}

impl MachinesView {
    fn item_title(item: &MorningItem, branch: Option<&str>, a: Appearance) -> Div {
        let line: Vec<&str> = [Some(item.machine.as_str()), branch]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(3.0))
            .child(label(
                &format!("{} #{}", item.repo, item.issue),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ))
            .child(text_line(&item.title, TypeRole::Heading, Ink::Text, a))
            .child(label(&line.join(" · "), TypeRole::Meta, Ink::TextFaint, a))
    }

    fn action_status(&self, item: &MorningItem, a: Appearance) -> Div {
        let actions = &self.model.parts.actions;
        let mut column = div().flex().flex_col().gap(px(4.0));
        if actions.is_running(&item.id) {
            column = column.child(busy_line(
                SharedString::from(format!("working-{}", item.id)),
                "Working…",
                TypeRole::Label,
                Ink::TextMuted,
                a,
            ));
        }
        let error = actions.last_error(&item.id);
        if let Some(error) = &error {
            column = column.child(text_line(error, TypeRole::Meta, Ink::Expired, a));
        }
        let note = self
            .model
            .done
            .get(&item.id)
            .cloned()
            .or_else(|| actions.note(&item.id));
        if let (Some(note), None) = (note, error) {
            column = column.child(label(&format!("✓ {note}"), TypeRole::Label, Ink::Done, a));
        }
        column
    }

    fn note_field(
        &self,
        item_id: &str,
        placeholder: &str,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let text = self.notes.get(item_id).cloned().unwrap_or_default();
        let focused = self.typing.as_deref() == Some(item_id);
        let id = item_id.to_owned();
        div()
            .id(SharedString::from(format!("note-{item_id}")))
            .flex_1()
            .min_w(px(0.0))
            .cursor_text()
            .child(field(&text, placeholder, focused, a).w_full())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.typing = Some(id.clone());
                window.focus(&this.focus);
                cx.notify();
            }))
    }

    pub(super) fn send_note(&mut self, item_id: &str, cx: &mut Context<Self>) {
        let text = self
            .notes
            .get(item_id)
            .map(|t| t.trim().to_owned())
            .unwrap_or_default();
        let Some(item) = self.model.items.iter().find(|i| i.id == item_id).cloned() else {
            return;
        };
        if text.is_empty() || !self.model.may_act(&item) {
            return;
        }
        self.notes.remove(item_id);
        self.sending_back.remove(item_id);
        self.typing = None;
        let i = item.clone();
        match item.kind {
            MorningKind::Ready => self.perform(
                &item,
                Some("Sent back to the worker"),
                move |acts| acts.send_back(&i, &text),
                cx,
            ),
            MorningKind::NeedsYou => self.perform(
                &item,
                Some("Answered. The worker resumes it on its next tick"),
                move |acts| acts.answer(&i, &text),
                cx,
            ),
            MorningKind::Stuck => {}
        }
    }

    pub(super) fn ready_card(
        &self,
        item: &MorningItem,
        entry: &ReadyEntry,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let busy = !self.model.may_act(item);
        let outcome: Option<MergeOutcome> = self.model.parts.actions.outcome(item);
        let bid = |part: &str| SharedString::from(format!("{part}-{}", item.id));
        let (i1, i2, i3, i4, i5, i6) = (
            item.clone(),
            item.clone(),
            item.clone(),
            item.clone(),
            item.clone(),
            item.clone(),
        );
        let mut buttons = div()
            .flex()
            .flex_wrap()
            .gap(px(8.0))
            .child(
                button(bid("merge"), "Merge", ButtonKind::Primary, state(busy), a).when(
                    !busy,
                    |b| {
                        b.on_click(cx.listener(move |this, _, _, cx| {
                            let i = i1.clone();
                            this.perform(
                                &i1,
                                Some("Merged, pushed and closed"),
                                move |acts| acts.merge(&i),
                                cx,
                            )
                        }))
                    },
                ),
            )
            .child(
                button(
                    bid("diff"),
                    "Diff",
                    ButtonKind::Secondary,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.show_diff(&i2, cx))),
            )
            .child(
                button(
                    bid("send-back"),
                    "Send back",
                    ButtonKind::Secondary,
                    state(busy),
                    a,
                )
                .when(!busy, |b| {
                    b.on_click(cx.listener(move |this, _, window, cx| {
                        if !this.sending_back.remove(&i3.id) {
                            this.sending_back.insert(i3.id.clone());
                            this.typing = Some(i3.id.clone());
                            window.focus(&this.focus);
                        }
                        cx.notify();
                    }))
                }),
            )
            .child(
                button(bid("close"), "Close", ButtonKind::Ghost, state(busy), a).when(!busy, |b| {
                    b.on_click(cx.listener(move |this, _, _, cx| {
                        this.confirming = Some(Confirming::Close(i4.clone()));
                        cx.notify();
                    }))
                }),
            );
        if outcome.as_ref().is_some_and(MergeOutcome::can_retry_push) {
            buttons = buttons.child(
                button(
                    bid("retry-push"),
                    "Retry push",
                    ButtonKind::Primary,
                    state(busy),
                    a,
                )
                .when(!busy, |b| {
                    b.on_click(cx.listener(move |this, _, _, cx| {
                        let i = i5.clone();
                        this.perform(
                            &i5,
                            Some("Pushed and closed"),
                            move |acts| acts.retry_push(&i),
                            cx,
                        )
                    }))
                }),
            );
        }
        if outcome.as_ref().is_some_and(MergeOutcome::can_undo) {
            buttons = buttons.child(
                button(
                    bid("undo-merge"),
                    "Undo merge",
                    ButtonKind::Secondary,
                    state(busy),
                    a,
                )
                .when(!busy, |b| {
                    b.on_click(cx.listener(move |this, _, _, cx| {
                        let i = i6.clone();
                        this.perform(&i6, None, move |acts| acts.undo_merge(&i), cx)
                    }))
                }),
            );
        }
        let mut body = card(a)
            .w_full()
            .p(px(CARD_PADDING))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(Self::item_title(item, Some(&entry.branch), a));
        if let Some(comment) = &item.comment {
            body = body.child(text_line(comment, TypeRole::Body, Ink::Text, a));
        }
        body = body.child(buttons);
        if self.sending_back.contains(&item.id) {
            let id = item.id.clone();
            let can_send = !busy
                && self
                    .notes
                    .get(&item.id)
                    .is_some_and(|t| !t.trim().is_empty());
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(self.note_field(&item.id, "What should the worker change?", a, cx))
                    .child(
                        button(
                            bid("send"),
                            "Send",
                            ButtonKind::Primary,
                            state(!can_send),
                            a,
                        )
                        .when(can_send, |b| {
                            b.on_click(cx.listener(move |this, _, _, cx| this.send_note(&id, cx)))
                        }),
                    ),
            );
        }
        if let Some(outcome) = &outcome {
            body = body.child(checklist(outcome, a));
        }
        body.child(self.action_status(item, a))
    }

    pub(super) fn needs_you_card(
        &self,
        item: &MorningItem,
        entry: &NeedsYouEntry,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let busy = !self.model.may_act(item);
        let bid = |part: &str| SharedString::from(format!("{part}-{}", item.id));
        let recommend = entry.recommend.as_deref().filter(|r| !r.is_empty());
        let mut body = card(a)
            .w_full()
            .p(px(CARD_PADDING))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(Self::item_title(item, entry.branch.as_deref(), a));
        match entry.question.as_deref().filter(|q| !q.is_empty()) {
            Some(question) => {
                body = body.child(text_line(question, TypeRole::Body, Ink::Text, a));
                if !entry.reason.is_empty() {
                    body = body.child(text_line(&entry.reason, TypeRole::Meta, Ink::TextMuted, a));
                }
            }
            None if !entry.reason.is_empty() => {
                body = body.child(text_line(&entry.reason, TypeRole::Body, Ink::Text, a));
            }
            None => {}
        }
        if let Some(r) = recommend {
            body = body.child(label(
                &format!("Recommended: {r}"),
                TypeRole::Label,
                Ink::AccentText,
                a,
            ));
        }
        if !entry.options.is_empty() {
            let mut options = div().flex().flex_wrap().gap(px(8.0));
            for (k, option) in entry.options.iter().enumerate() {
                let (i, text) = (item.clone(), option.clone());
                options = options.child(
                    button(
                        bid(&format!("option-{k}")),
                        option,
                        ButtonKind::Secondary,
                        state(busy),
                        a,
                    )
                    .when(!busy, |b| {
                        b.on_click(cx.listener(move |this, _, _, cx| {
                            let (i2, t) = (i.clone(), text.clone());
                            this.perform(
                                &i,
                                Some("Answered. The worker resumes it on its next tick"),
                                move |acts| acts.answer(&i2, &t),
                                cx,
                            )
                        }))
                    }),
                );
            }
            body = body.child(options);
        }
        let id = item.id.clone();
        let can_send = !busy
            && self
                .notes
                .get(&item.id)
                .is_some_and(|t| !t.trim().is_empty());
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(self.note_field(&item.id, "Your answer", a, cx))
                .child(
                    button(
                        bid("answer"),
                        "Answer",
                        ButtonKind::Primary,
                        state(!can_send),
                        a,
                    )
                    .when(can_send, |b| {
                        b.on_click(cx.listener(move |this, _, _, cx| this.send_note(&id, cx)))
                    }),
                ),
        );
        let (i1, i2) = (item.clone(), item.clone());
        let mut row = div().flex().gap(px(8.0));
        if recommend.is_some() {
            row = row.child(
                button(
                    bid("use-recommended"),
                    "Use recommended",
                    ButtonKind::Secondary,
                    state(busy),
                    a,
                )
                .when(!busy, |b| {
                    b.on_click(cx.listener(move |this, _, _, cx| {
                        let i = i1.clone();
                        this.perform(
                            &i1,
                            Some("Answered with the recommendation"),
                            move |acts| acts.use_recommended(&i),
                            cx,
                        )
                    }))
                }),
            );
        }
        row = row.child(
            button(bid("close"), "Close", ButtonKind::Ghost, state(busy), a).when(!busy, |b| {
                b.on_click(cx.listener(move |this, _, _, cx| {
                    this.confirming = Some(Confirming::Close(i2.clone()));
                    cx.notify();
                }))
            }),
        );
        body.child(row).child(self.action_status(item, a))
    }

    pub(super) fn stuck_card(
        &self,
        item: &MorningItem,
        entry: &StuckEntry,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let busy = !self.model.may_act(item);
        let bid = |part: &str| SharedString::from(format!("{part}-{}", item.id));
        let mut body = card(a)
            .w_full()
            .p(px(CARD_PADDING))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(Self::item_title(item, entry.branch.as_deref(), a));
        if !entry.reason.is_empty() {
            body = body.child(text_line(&entry.reason, TypeRole::Body, Ink::Text, a));
        }
        if !entry.report_tail.is_empty() {
            body = body
                .child(label("Report tail", TypeRole::Label, Ink::TextMuted, a))
                .child(mono_box(
                    format!("report-{}", item.id),
                    &entry.report_tail,
                    180.0,
                    a,
                ));
        }
        let (i1, i2) = (item.clone(), item.clone());
        body = body.child(
            div()
                .flex()
                .gap(px(8.0))
                .child(
                    button(bid("retry"), "Retry", ButtonKind::Primary, state(busy), a).when(
                        !busy,
                        |b| {
                            b.on_click(cx.listener(move |this, _, _, cx| {
                                let i = i1.clone();
                                this.perform(
                                    &i1,
                                    Some("Queued again"),
                                    move |acts| acts.retry(&i),
                                    cx,
                                )
                            }))
                        },
                    ),
                )
                .child(
                    button(bid("drop"), "Drop", ButtonKind::Ghost, state(busy), a).when(
                        !busy,
                        |b| {
                            b.on_click(cx.listener(move |this, _, _, cx| {
                                this.confirming = Some(Confirming::Drop(i2.clone()));
                                cx.notify();
                            }))
                        },
                    ),
                ),
        );
        body.child(self.action_status(item, a))
    }
}

fn checklist(outcome: &MergeOutcome, a: Appearance) -> Div {
    let steps = [
        ("fetched", outcome.fetched),
        ("merged", outcome.merged),
        ("checked", outcome.checked),
        ("pushed", outcome.pushed),
        ("closed", outcome.closed),
    ];
    let mut row = div().flex().gap(px(14.0));
    for (name, ok) in steps {
        row = row.child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(icon(
                    if ok { Icon::Check } else { Icon::Circle },
                    TypeRole::Meta.spec().size,
                    if ok { Ink::Done } else { Ink::TextFaint },
                    a,
                ))
                .child(label(
                    name,
                    TypeRole::Meta,
                    if ok { Ink::Text } else { Ink::TextMuted },
                    a,
                )),
        );
    }
    row
}
