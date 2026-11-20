use gpui::{AppContext, Context, Entity, Subscription};
use notchling_kit::ui::{TextEditor, TextEditorEvent};

use super::answer::QuestionDraft;

pub struct AnswerEditor {
    editor: Entity<TextEditor>,
    at: Option<(String, usize)>,
    _events: Subscription,
}

impl AnswerEditor {
    pub fn new<V: 'static>(
        placeholder: &str,
        cx: &mut Context<V>,
        on: impl Fn(&mut V, &TextEditorEvent, &mut Context<V>) + 'static,
    ) -> AnswerEditor {
        let editor = cx.new(|cx| TextEditor::new(placeholder, 1, cx).single_line());
        let events = cx.subscribe(&editor, move |this, _, event: &TextEditorEvent, cx| {
            on(this, event, cx)
        });
        AnswerEditor {
            editor,
            at: None,
            _events: events,
        }
    }

    pub fn editor(&self) -> &Entity<TextEditor> {
        &self.editor
    }

    pub fn show<V: 'static>(&mut self, draft: &QuestionDraft, enabled: bool, cx: &mut Context<V>) {
        let at = (draft.id().to_owned(), draft.tab);
        let moved = self.at.as_ref() != Some(&at);
        let text = draft.shown_text(draft.tab);
        self.at = Some(at);
        self.editor.update(cx, |editor, cx| {
            editor.set_enabled(enabled, cx);
            if moved {
                editor.set_text(&text, cx);
            }
        });
    }
}
