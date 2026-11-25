use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_engine::vault::entry::VaultEntry;
use notchling_kit::metric::ICON_BUTTON_SMALL;
use notchling_kit::ui::{label, KitStyled, MONO_FONT};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::copy_name;
use super::list::Row;
use super::lock_state::reads_usernames;
use super::reads::Intent;
use super::reveal::{clears_clipboard, display, hides, FormShown, Part, Slot, Word};
use super::reveal_view::{eye_button, notify_when_hidden};
use super::view::VaultSheetView;
use super::{clip, lock_watch, reveal};
use crate::switches::Switch;

pub(super) struct FieldLine {
    pub slot: Slot,
    pub text: String,
    pub shown: bool,
    pub copied: bool,
}

fn copy_key(name: &str, part: Part) -> String {
    format!("\u{0}{part:?}\u{0}{name}")
}

fn word_label(word: Word) -> &'static str {
    match word {
        Word::Username => t::consent::card_vault_label_username(),
        Word::Password => t::consent::card_vault_label_password(),
        Word::Token => t::consent::card_vault_label_token(),
    }
}

fn copy_tip(word: Word) -> &'static str {
    match word {
        Word::Username => t::consent::card_vault_copy_username(),
        Word::Password => t::consent::card_vault_copy_password(),
        Word::Token => t::consent::card_vault_copy_token(),
    }
}

impl VaultSheetView {
    pub(super) fn field_lines(&self, row: &Row) -> Vec<FieldLine> {
        if !self.can_read() {
            return Vec::new();
        }
        let now = copy_name::now_ms();
        row.slots
            .iter()
            .map(|slot| {
                let shown = slot.secret() && self.revealed.is_shown(&row.name, Part::Secret);
                let text = match slot.part {
                    Part::Username => self
                        .usernames
                        .get(&row.name)
                        .and_then(|(_, name)| name.clone())
                        .unwrap_or_default(),
                    Part::Secret => display(self.revealed.value(&row.name, Part::Secret, now)),
                };
                FieldLine {
                    slot: *slot,
                    text,
                    shown: shown && self.revealed.value(&row.name, Part::Secret, now).is_some(),
                    copied: copy_name::shows(&self.copied, &copy_key(&row.name, slot.part)),
                }
            })
            .collect()
    }

    pub(super) fn can_read(&self) -> bool {
        self.store().is_some_and(|v| v.can_read()) && self.switches().is_on(Switch::Vault)
    }

    pub(super) fn guard_reveals(&mut self, entries: &[VaultEntry], cx: &mut Context<Self>) {
        let hide = hides(self.can_read(), self.switches().is_on(Switch::Vault));
        if hide.form {
            self.form_shown = FormShown::hidden();
        }
        if hide.reads {
            self.hide_reads();
            return;
        }
        let current: Vec<(&str, String)> = entries
            .iter()
            .map(|e| (e.name.as_str(), e.revision()))
            .collect();
        self.revealed.keep_current(&current);
        self.reads.keep_current(&current);
        let with_username: Vec<&str> = entries
            .iter()
            .filter(|e| has_username(e))
            .map(|e| e.name.as_str())
            .collect();
        self.usernames.retain(|name, (rev, _)| {
            with_username.contains(&name.as_str())
                && current.iter().any(|(n, r)| *n == name.as_str() && r == rev)
        });
        if !reads_usernames(self.form.is_some(), lock_watch::is_locked()) {
            return;
        }
        for (entry, (_, rev)) in entries.iter().zip(&current) {
            if !has_username(entry)
                || self
                    .usernames
                    .get(&entry.name)
                    .is_some_and(|(seen, _)| seen == rev)
            {
                continue;
            }
            let rev = rev.clone();
            let (name, at) = (entry.name.clone(), rev.clone());
            let started = self.read_part(
                &entry.name,
                Part::Username,
                Intent::Cache,
                &rev,
                cx,
                move |view, result, _| {
                    if let Ok(value) = result {
                        view.usernames.insert(name, (at, Some(value)));
                    }
                },
            );
            if started {
                self.usernames.insert(entry.name.clone(), (rev, None));
            }
        }
    }

    pub(super) fn drop_pending(&mut self) {
        self.reads.cancel_all();
        self.usernames.retain(|_, (_, value)| value.is_some());
    }

    pub(super) fn hide_reads(&mut self) {
        self.revealed.hide_all();
        self.usernames.clear();
        self.reads.cancel_all();
    }

    pub(super) fn hide_everything(&mut self) {
        self.hide_reads();
        self.form_shown = FormShown::hidden();
    }

    fn read_part(
        &mut self,
        name: &str,
        part: Part,
        intent: Intent,
        rev: &str,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<String, String>, &mut Context<Self>) + 'static,
    ) -> bool {
        let Some(vault) = self.store().filter(|v| v.can_read()) else {
            return false;
        };
        let Some(ticket) = self
            .reads
            .begin(name, part, intent, rev, lock_watch::unlocked())
        else {
            return false;
        };
        let source = self.switch_source();
        let name = name.to_owned();
        let at = rev.to_owned();
        cx.spawn(async move |this, cx| {
            let target = name.clone();
            let result = cx
                .background_executor()
                .spawn(async move { vault.read_part(&source.get(), &target, part, &at) })
                .await;
            this.update(cx, |view, cx| {
                let current = view.entry(&name).map(|e| e.revision());
                let landed = view.reads.land(
                    &name,
                    part,
                    intent,
                    &ticket,
                    current.as_deref(),
                    lock_watch::unlocked(),
                );
                if landed && view.can_read() {
                    done(view, result, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        true
    }

    pub(super) fn toggle_eye(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.revealed.is_shown(name, Part::Secret)
            || self.reads.is_open(name, Part::Secret, Intent::Eye)
        {
            self.revealed.hide(name, Part::Secret);
            self.reads.cancel(name, Part::Secret, Intent::Eye);
            cx.notify();
            return;
        }
        let Some(rev) = self.entry(name).map(|e| e.revision()) else {
            return;
        };
        let (key, at) = (name.to_owned(), rev.clone());
        self.note = None;
        self.read_part(
            name,
            Part::Secret,
            Intent::Eye,
            &rev,
            cx,
            move |view, result, cx| match result {
                Ok(value) => {
                    view.revealed
                        .show(&key, Part::Secret, &at, value, copy_name::now_ms());
                    notify_when_hidden(cx);
                }
                Err(error) => view.note = Some(error),
            },
        );
    }

    pub(super) fn copy_part(&mut self, name: &str, part: Part, cx: &mut Context<Self>) {
        let Some(rev) = self.entry(name).map(|e| e.revision()) else {
            return;
        };
        let key = copy_key(name, part);
        self.note = None;
        self.read_part(
            name,
            part,
            Intent::Copy,
            &rev,
            cx,
            move |view, result, cx| match result {
                Ok(value) => {
                    let copied = if clears_clipboard(part) {
                        clip::copy_secret(&value, cx)
                    } else {
                        clip::copy_plain(&value, cx);
                        true
                    };
                    if copied {
                        copy_name::mark(&mut view.copied, &key, cx);
                    } else {
                        view.note = Some(t::consent::card_vault_copy_failed().to_owned());
                    }
                }
                Err(error) => view.note = Some(error),
            },
        );
    }

    pub(super) fn sweep_reveals(&mut self, cx: &mut Context<Self>) -> bool {
        let changed = if lock_watch::moved(&mut self.seen_lock) {
            self.hide_everything();
            true
        } else {
            let now = copy_name::now_ms();
            let swept = self.revealed.sweep(now);
            let form = self.form_shown.sweep(now);
            let hid_form = form != self.form_shown;
            self.form_shown = form;
            swept || hid_form
        };
        if changed {
            self.show_form_secret(cx);
        }
        changed
    }
}

fn has_username(entry: &VaultEntry) -> bool {
    reveal::slots(entry.kind)
        .iter()
        .any(|s| s.part == Part::Username)
}

pub(super) fn field_row(
    name: &str,
    line: &FieldLine,
    a: Appearance,
    cx: &mut Context<VaultSheetView>,
) -> Div {
    let slot = line.slot;
    let ink = if slot.secret() && !line.shown {
        Ink::TextFaint
    } else {
        Ink::Text
    };
    let value = div()
        .flex_1()
        .min_w(px(0.0))
        .font_family(MONO_FONT)
        .text_size(px(11.0))
        .ink(ink, a)
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(line.text.clone());
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            label(word_label(slot.word), TypeRole::Meta, Ink::TextFaint, a)
                .w(px(64.0))
                .flex_shrink_0(),
        )
        .child(value);
    if slot.eye {
        let n = name.to_owned();
        row = row.child(
            eye_button(
                SharedString::from(format!("vault-eye-{name}")),
                line.shown,
                true,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_eye(&n, cx))),
        );
    }
    if !slot.eye {
        row = row.child(div().size(px(ICON_BUTTON_SMALL)).flex_shrink_0());
    }
    if slot.copy {
        let n = name.to_owned();
        let part = slot.part;
        row = row.child(
            copy_name::button_tip(
                SharedString::from(format!("vault-copy-{part:?}-{name}")),
                line.copied,
                true,
                copy_tip(slot.word),
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.copy_part(&n, part, cx))),
        );
    }
    row
}
