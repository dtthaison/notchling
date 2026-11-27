use gpui::{div, prelude::*, px, Context, Div, Entity, SharedString, Subscription, Window};
use notchling_engine::inbox::NOTE_MAX;
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, editor_field, label, TextEditor, TextEditorEvent};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::model::ChangeRoute;
use super::triage::change_busy;
use super::view::InboxPanelView;

const COUNT_FROM: usize = 250;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ChangeOpen {
    pub id: String,
    pub route: ChangeRoute,
}

pub(super) struct NoteEditor {
    pub editor: Entity<TextEditor>,
    _events: Subscription,
}

pub(super) fn can_send(note: &str) -> bool {
    let chars = note.trim().chars().count();
    chars > 0 && chars <= NOTE_MAX
}

impl InboxPanelView {
    fn note_editor(&mut self, cx: &mut Context<Self>) -> Entity<TextEditor> {
        if let Some(held) = &self.editor {
            return held.editor.clone();
        }
        let editor =
            cx.new(|cx| TextEditor::new(t::consent::change_placeholder(), 1, cx).single_line());
        let events = cx.subscribe(
            &editor,
            |this, _, event: &TextEditorEvent, cx| match event {
                TextEditorEvent::Changed => cx.notify(),
                TextEditorEvent::Submit => this.send_change(cx),
            },
        );
        self.editor = Some(NoteEditor {
            editor: editor.clone(),
            _events: events,
        });
        editor
    }

    pub(super) fn open_change(
        &mut self,
        id: String,
        route: ChangeRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = self.note_editor(cx);
        editor.update(cx, |editor, cx| editor.set_text("", cx));
        self.change = Some(ChangeOpen { id, route });
        editor.read(cx).focus(window);
        cx.notify();
    }

    pub(super) fn cancel_change(&mut self, cx: &mut Context<Self>) {
        self.change = None;
        cx.notify();
    }

    pub(super) fn prepare_change(&mut self, cx: &mut Context<Self>) {
        if self.change.is_none() {
            return;
        }
        let editor = self.note_editor(cx);
        if let Some(text) = self.preset.take() {
            editor.update(cx, |editor, cx| editor.set_text(&text, cx));
        }
    }

    pub(super) fn send_change(&mut self, cx: &mut Context<Self>) {
        let (Some(open), Some(held)) = (self.change.clone(), self.editor.as_ref()) else {
            return;
        };
        let note = held.editor.read(cx).text().trim().to_owned();
        if !can_send(&note) || change_busy(&self.busy, &open.id, &open.route) {
            return;
        }
        let keys = busy_keys(&open);
        self.busy.extend(keys.iter().cloned());
        let feed = self.feed.clone();
        let route = open.route.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match route {
                        ChangeRoute::Consent(id) => feed.consent_deny_with(&id, &note),
                        ChangeRoute::Approval(ticket) => feed.approval_deny_with(&ticket, &note),
                    }
                })
                .await;
            this.update(cx, |view, cx| {
                for key in &keys {
                    view.busy.remove(key);
                }
                match result {
                    Ok(_) => view.change = None,
                    Err(e) => view.fail(&open.id, e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn change_field(
        &self,
        row_id: &str,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let open = self.change.as_ref().filter(|open| open.id == row_id)?;
        let held = self.editor.as_ref()?;
        let (text, focused) = {
            let editor = held.editor.read(cx);
            (editor.text().to_owned(), editor.is_focused(window))
        };
        let chars = text.trim().chars().count();
        let busy = change_busy(&self.busy, &open.id, &open.route);
        let send_state = if can_send(&text) && !busy {
            PartState::Rest
        } else {
            PartState::Disabled
        };
        let send = button(
            SharedString::from(format!("inbox-change-send-{row_id}")),
            t::consent::change_send(),
            ButtonKind::Primary,
            send_state,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| this.send_change(cx)));
        let cancel = button(
            SharedString::from(format!("inbox-change-cancel-{row_id}")),
            t::consent::change_cancel(),
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| this.cancel_change(cx)));
        let counter = (chars > COUNT_FROM).then(|| {
            let ink = if chars > NOTE_MAX {
                Ink::Needs
            } else {
                Ink::TextFaint
            };
            label(
                &t::inbox::change_count(&chars.to_string(), &NOTE_MAX.to_string()),
                TypeRole::Meta,
                ink,
                a,
            )
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(SPACE[1]))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(SPACE[1]))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(editor_field(&held.editor, focused, a).w_full()),
                        )
                        .child(send)
                        .child(cancel),
                )
                .child(label(
                    t::inbox::change_hint(),
                    TypeRole::Meta,
                    Ink::TextFaint,
                    a,
                ))
                .children(counter),
        )
    }
}

fn busy_keys(open: &ChangeOpen) -> Vec<String> {
    let mut keys = vec![open.id.clone()];
    if let ChangeRoute::Consent(id) = &open.route {
        keys.push(id.clone());
    }
    keys
}
