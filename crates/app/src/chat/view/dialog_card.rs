use std::collections::HashSet;

use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_kit::metric::MENU_GAP;
use notchling_kit::part::ButtonKind;
use notchling_kit::ui::{button, label, option_row_plain};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::prompts::{failed, header, status};
use super::ChatView;
use crate::acting::{Control, Request};
use crate::chat::dialog_card::{asked_by_outbox, is_busy, read_key, DialogCard, KEY_PREFIX};
use crate::chat::prompt_flow::{Sending, Told};
use crate::chat::prompts::Prompt;
use crate::mac::gate;
use crate::phone::bridge::now_ms;

impl ChatView {
    pub(super) fn dialog_key(&self) -> Option<String> {
        let session = self.selected_session()?;
        let asked = asked_by_outbox(&self.source.outbox_rows(&session.agent));
        read_key(
            &session.agent,
            session.tool,
            asked,
            self.prompt.dialog.round(),
        )
    }

    pub(super) fn dialog_prompt(&self, answered: &HashSet<String>) -> Option<Prompt> {
        let key = self.dialog_key().filter(|key| !answered.contains(key))?;
        let card = self.prompt.dialog.shown(&key)?;
        Some(Prompt::Dialog(Box::new(card.clone())))
    }

    pub(super) fn dialog_card_shown(&self) -> bool {
        matches!(self.prompt.shown, Some(Prompt::Dialog(_)))
    }

    pub(super) fn reread_dialog_after(&mut self, key: &str) {
        if key.starts_with(KEY_PREFIX) {
            self.prompt.dialog.next_round();
        }
    }

    pub(super) fn dismiss_session_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_session().map(|s| s.agent.id.clone()) else {
            return;
        };
        self.spawn_send(
            Request::dialog_dismiss(&id),
            |view, result, _| {
                if let Err(message) = result {
                    view.tools.error = Some(message);
                }
            },
            cx,
        );
    }

    pub(super) fn read_screen_dialog(&mut self, cx: &mut Context<Self>) {
        if self.prompt.preview.is_some() || self.root.fixture.is_some() {
            return;
        }
        let Some(key) = self.dialog_key() else {
            return;
        };
        let (_, answered) = self.source.held();
        if answered.contains(&key)
            || !self.prompt.dialog.due(&key, now_ms())
            || self.prompt_now().is_some()
            || !self.acting(cx).control().enabled
        {
            return;
        }
        let Some(row) = self.selected_session().map(|s| s.agent.id.clone()) else {
            return;
        };
        self.prompt.dialog.begin(&key);
        let actions = self.acts(cx).actions;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&Request::dialog(&row)) })
                .await;
            this.update(cx, |view, cx| {
                let busy = result.as_ref().is_ok_and(is_busy);
                let card = result
                    .ok()
                    .and_then(|reply| DialogCard::from_reply(&key, &reply));
                match busy {
                    true => view.prompt.dialog.busy(&key, now_ms()),
                    false => view.prompt.dialog.finish(&key, card, now_ms()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

pub(super) fn content(
    view: &ChatView,
    row: &str,
    card: &DialogCard,
    gate: Control,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let sending = view.prompt.is_sending();
    let options = card
        .options
        .iter()
        .map(|option| {
            let picked = sending && view.prompt.dialog.picked == Some(option.number);
            let item = option_row_plain(
                SharedString::from(format!("prompt-dialog-{}", option.number)),
                usize::try_from(option.number).unwrap_or(0),
                &option.label,
                picked,
                gate::state(gate),
                a,
            );
            let (key, request) = (card.key.clone(), card.pick(row, option));
            let (number, label_text) = (option.number, option.label.clone());
            let said = Told {
                went_through: t::chat::dialog_chose(&label_text),
                already_answered: t::chat::dialog_chose_gone().to_owned(),
            };
            gate::tip(item.w_auto(), gate, a).when(gate.enabled && !sending, |el| {
                el.on_click(cx.listener(move |this, _, _, cx| {
                    this.prompt.dialog.picked = Some(number);
                    this.answer_prompt(
                        key.clone(),
                        request.clone(),
                        Sending::Answer,
                        Some(said.clone()),
                        cx,
                    );
                }))
            })
        })
        .collect::<Vec<_>>();
    let open = Control {
        enabled: gate.enabled && !sending,
        tooltip: gate.tooltip,
    };
    let (key, request) = (card.key.clone(), card.dismiss(row));
    let dismiss = gate::tip(
        button(
            "prompt-dialog-dismiss",
            t::chat::dialog_dismiss(),
            ButtonKind::Ghost,
            gate::state(open),
            a,
        ),
        open,
        a,
    )
    .when(open.enabled, |el| {
        el.on_click(cx.listener(move |this, _, _, cx| {
            let told = Told {
                went_through: t::chat::dialog_dismissed().to_owned(),
                already_answered: t::chat::dialog_chose_gone().to_owned(),
            };
            this.answer_prompt(
                key.clone(),
                request.clone(),
                Sending::Answer,
                Some(told),
                cx,
            );
        }))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(MENU_GAP))
        .child(header(
            Icon::Question,
            t::chat::claude_asks(),
            Ink::Working,
            None,
            a,
        ))
        .child(label(&card.title, TypeRole::Heading, Ink::Text, a))
        .children(
            card.body
                .iter()
                .map(|line| label(line, TypeRole::Meta, Ink::TextMuted, a)),
        )
        .children(failed(view, a, cx))
        .child(div().flex().flex_col().children(options))
        .child(div().flex().justify_end().child(dismiss))
        .children(status(view, gate, a))
}
