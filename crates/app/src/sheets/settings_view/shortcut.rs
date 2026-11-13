use gpui::{div, prelude::*, px, Context, Div, Subscription};
use notchling_kit::metric::RADIUS_SM;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, label, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};
use serde_json::json;

use super::SettingsView;
use crate::mac::shell;
use crate::search::shortcut::{Mods, Setting, Shortcut, KEY};

const RECORDING: &str = "Press a shortcut…";

#[derive(Default)]
pub(super) struct ShortcutState {
    recording: Option<Subscription>,
    note: Option<(String, bool)>,
}

impl SettingsView {
    pub(super) fn shortcut_field(
        &self,
        setting: Setting,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let (title, kind) = match (self.shortcut.recording.is_some(), setting) {
            (true, _) => (RECORDING.to_owned(), ButtonKind::Primary),
            (false, Setting::On(key)) => (key.label(), ButtonKind::Secondary),
            (false, Setting::Off) => ("Record".to_owned(), ButtonKind::Ghost),
        };
        let off_kind = if setting == Setting::Off {
            ButtonKind::Secondary
        } else {
            ButtonKind::Ghost
        };
        div()
            .flex()
            .p(px(2.0))
            .gap(px(2.0))
            .rounded(px(RADIUS_SM))
            .fill(Ink::Raised, a)
            .child(
                button("setting-shortcut-record", &title, kind, PartState::Rest, a)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_recording(cx)))
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| this.stop_recording(cx))),
            )
            .child(
                button("setting-shortcut-off", "Off", off_kind, PartState::Rest, a)
                    .on_click(cx.listener(|this, _, _, cx| this.set_shortcut(Setting::Off, cx))),
            )
    }

    pub(super) fn shortcut_note(&self, a: Appearance, cx: &Context<Self>) -> Option<Div> {
        let (text, bad) = match &self.shortcut.note {
            Some((text, bad)) => (text.clone(), *bad),
            None => (shell::search_failure(cx)?, true),
        };
        let ink = if bad { Ink::Needs } else { Ink::TextFaint };
        Some(label(&text, TypeRole::Meta, ink, a))
    }

    fn toggle_recording(&mut self, cx: &mut Context<Self>) {
        if self.shortcut.recording.take().is_some() {
            cx.notify();
            return;
        }
        self.shortcut.note = None;
        let view = cx.entity().downgrade();
        self.shortcut.recording = Some(cx.intercept_keystrokes(move |event, _, cx| {
            cx.stop_propagation();
            let key = &event.keystroke;
            let mods = Mods {
                ctrl: key.modifiers.control,
                alt: key.modifiers.alt,
                shift: key.modifiers.shift,
                cmd: key.modifiers.platform,
            };
            let name = key.key.clone();
            view.update(cx, |this, cx| this.recorded(&name, mods, cx))
                .ok();
        }));
        cx.notify();
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if self.shortcut.recording.take().is_some() {
            cx.notify();
        }
    }

    fn recorded(&mut self, key: &str, mods: Mods, cx: &mut Context<Self>) {
        if key == "escape" && mods == Mods::default() {
            self.stop_recording(cx);
            return;
        }
        match Shortcut::from_keystroke(key, mods) {
            Some(shortcut) => self.set_shortcut(Setting::On(shortcut), cx),
            None => {
                self.shortcut.note = Some((
                    "Use a letter, a number or Space with Control, Option or Command.".to_owned(),
                    true,
                ));
                cx.notify();
            }
        }
    }

    fn set_shortcut(&mut self, setting: Setting, cx: &mut Context<Self>) {
        self.shortcut.recording = None;
        let before = shell::active_search(cx);
        if let Setting::On(key) = setting {
            if let Some(problem) = key.problem() {
                self.shortcut.note = Some((problem.to_owned(), true));
                cx.notify();
                return;
            }
        }
        if let Err(e) = shell::set_search_shortcut(cx, setting) {
            let kept = match before {
                Setting::On(old) => format!(" {} stays.", old.label()),
                Setting::Off => String::new(),
            };
            self.shortcut.note = Some((format!("{e}{kept}"), true));
            cx.notify();
            return;
        }
        let value = json!(setting.value());
        if let Some(store) = &mut self.store {
            if let Err(e) = store.set(KEY, value.clone()) {
                self.error = Some(e.to_string());
            }
        }
        self.values.insert(KEY.to_owned(), value.clone());
        self.shortcut.note = match setting {
            Setting::On(key) => key.clash().map(|c| (c.to_owned(), false)),
            Setting::Off => None,
        };
        cx.emit(super::SettingChanged {
            key: KEY.to_owned(),
            value,
        });
        cx.notify();
    }
}
