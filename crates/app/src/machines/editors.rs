use std::cell::RefCell;
use std::collections::HashMap;

use gpui::{AppContext, Context, Entity, Subscription, Window};
use notchling_kit::ui::{TextEditor, TextEditorEvent};

use super::forms::{AddField, JobField};
use super::model::Sheet;
use super::view::MachinesView;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Key {
    Add(AddField),
    Job(JobField),
    Password,
    Note(String),
}

#[derive(Default)]
pub(super) struct Editors {
    map: RefCell<HashMap<Key, (Entity<TextEditor>, Subscription)>>,
    want: RefCell<Option<Key>>,
    now: RefCell<Option<Key>>,
}

impl Editors {
    pub(super) fn want(&self, key: Key) {
        *self.want.borrow_mut() = Some(key);
    }

    pub(super) fn forget_sheet(&self) {
        self.map
            .borrow_mut()
            .retain(|key, _| matches!(key, Key::Note(_)));
    }

    pub(super) fn retain_notes(&self, mut live: impl FnMut(&str) -> bool) {
        self.map.borrow_mut().retain(|key, _| match key {
            Key::Note(id) => live(id),
            _ => true,
        });
    }

    pub(super) fn forget_note(&self, id: &str) {
        self.map.borrow_mut().remove(&Key::Note(id.to_owned()));
    }

    fn get(&self, key: &Key) -> Option<Entity<TextEditor>> {
        self.map.borrow().get(key).map(|(editor, _)| editor.clone())
    }

    pub(super) fn focused(&self, window: &Window, cx: &gpui::App) -> Option<Key> {
        self.map
            .borrow()
            .iter()
            .find(|(_, (editor, _))| editor.read(cx).is_focused(window))
            .map(|(key, _)| key.clone())
    }

    pub(super) fn note_focus(&self, window: &Window, cx: &gpui::App) {
        *self.now.borrow_mut() = self.focused(window, cx);
    }

    pub(super) fn is_focus(&self, key: &Key) -> bool {
        self.now.borrow().as_ref() == Some(key)
    }

    pub(super) fn apply_want(&self, window: &mut Window, cx: &gpui::App) {
        let Some(key) = self.want.borrow().clone() else {
            return;
        };
        if let Some(editor) = self.get(&key) {
            editor.read(cx).focus(window);
            *self.want.borrow_mut() = None;
        }
    }
}

impl MachinesView {
    pub(super) fn editor(
        &self,
        key: Key,
        placeholder: &str,
        masked: bool,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Entity<TextEditor> {
        if let Some(editor) = self.editors.get(&key) {
            if editor.read(cx).text() != text {
                editor.update(cx, |editor, cx| editor.set_text(text, cx));
            }
            return editor;
        }
        let editor = cx.new(|cx| {
            let editor = TextEditor::new(placeholder, 1, cx).single_line();
            if masked {
                editor.masked()
            } else {
                editor
            }
        });
        editor.update(cx, |editor, cx| editor.set_text(text, cx));
        let at = key.clone();
        let events = cx.subscribe(&editor, move |this, editor, event: &TextEditorEvent, cx| {
            this.edited(&at, &editor, event, cx)
        });
        self.editors
            .map
            .borrow_mut()
            .insert(key, (editor.clone(), events));
        cx.notify();
        editor
    }

    fn edited(
        &mut self,
        key: &Key,
        editor: &Entity<TextEditor>,
        event: &TextEditorEvent,
        cx: &mut Context<Self>,
    ) {
        let text = editor.read(cx).text().to_owned();
        match key {
            Key::Add(field) => {
                if let Some(Sheet::Add(form)) = self.model.sheet_mut() {
                    form.set_text(*field, &text);
                }
            }
            Key::Job(field) => {
                if let Some(Sheet::Job(form)) = self.model.sheet_mut() {
                    form.set_text(*field, &text);
                }
            }
            Key::Password => {
                if let Some(password) = self.model.password_field() {
                    password.clear();
                    password.push_str(&text);
                }
            }
            Key::Note(id) => {
                if text.is_empty() {
                    self.notes.remove(id);
                } else {
                    self.notes.insert(id.clone(), text);
                }
                self.typing = Some(id.clone());
            }
        }
        if matches!(event, TextEditorEvent::Submit) {
            match key {
                Key::Note(id) => self.send_note(id, cx),
                _ => self.submit(cx),
            }
        }
        cx.notify();
    }

    pub(super) fn tab_sheet(&mut self, window: &Window, back: bool, cx: &gpui::App) -> bool {
        let at = self.editors.focused(window, cx);
        let next = match self.model.sheet_mut() {
            Some(Sheet::Add(form)) => {
                let from = match at {
                    Some(Key::Add(field)) => Some(field),
                    _ => None,
                };
                Some(Key::Add(if back {
                    form.tab_back_from(from)
                } else {
                    form.tab_from(from)
                }))
            }
            Some(Sheet::Job(form)) => {
                let from = match at {
                    Some(Key::Job(field)) => Some(field),
                    _ => None,
                };
                if back {
                    form.tab_back_from(from)
                } else {
                    form.tab_from(from)
                }
                .map(Key::Job)
            }
            _ => None,
        };
        match next {
            Some(key) => {
                self.editors.want(key);
                true
            }
            None => false,
        }
    }

    pub(super) fn want_sheet_focus(&self) {
        let key = match self.model.sheet() {
            Some(Sheet::Add(form)) => form.focus.map(Key::Add),
            Some(Sheet::Job(form)) => form.focus.map(Key::Job),
            Some(Sheet::Fix(prompt)) if prompt.needs_password() => Some(Key::Password),
            _ => None,
        };
        if let Some(key) = key {
            self.editors.want(key);
        }
    }
}
