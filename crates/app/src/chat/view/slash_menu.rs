//! The command list over the reply box (`chat::slash_menu` decides what it holds): each row the
//! command, its arguments' hint and what it does; the picked row raised. Its keys are taken before
//! the reply box sees them (`capture_key`), so Up and Down move in the list, not the text.

use gpui::{div, prelude::*, px, relative, Context, Div, KeyDownEvent, SharedString, Window};
use notchling_engine::control::slash::{self, SlashCommand};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::ui::{card, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::ChatView;
use crate::chat::data::Tool;
use crate::chat::slash_menu::{picked, query};

impl ChatView {
    /// The rows the list shows now: a Claude Code session's commands for what the box holds.
    pub(super) fn slash_rows(&self, cx: &gpui::App) -> Vec<SlashCommand> {
        let Some(session) = self.selected_session() else {
            return Vec::new();
        };
        if session.tool != Tool::Claude {
            return Vec::new();
        }
        let Some((cwd, commands)) = &self.slash_commands else {
            return Vec::new();
        };
        if *cwd != session.agent.cwd {
            return Vec::new();
        }
        let text = self.tools.editor.read(cx).text().to_owned();
        self.slash.rows(&text, commands)
    }

    /// After the box changed: the list starts again from its first row, and the session's commands
    /// are read once a "/" is typed in a folder not read yet.
    pub(super) fn slash_typed(&mut self, text: &str) {
        self.slash.typed(text);
        if query(text).is_none() {
            return;
        }
        let Some(cwd) = self.selected_session().map(|s| s.agent.cwd.clone()) else {
            return;
        };
        if self.slash_commands.as_ref().map(|(c, _)| c) != Some(&cwd) {
            let commands = slash::all(&self.root.home, &cwd);
            self.slash_commands = Some((cwd, commands));
        }
    }

    fn pick_slash(&mut self, command: &SlashCommand, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.put_draft(&id, picked(command), cx);
        self.redraw_chrome(cx);
    }

    /// Up, Down, Tab, Return and Esc while the list shows; anything else goes on to the box.
    pub(super) fn capture_slash_key(
        &mut self,
        event: &KeyDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !self.tools.focus.is_focused(window) {
            return;
        }
        let keys = &event.keystroke;
        let m = &keys.modifiers;
        if m.platform || m.control || m.alt {
            return;
        }
        let rows = self.slash_rows(cx);
        if rows.is_empty() {
            return;
        }
        match keys.key.as_str() {
            "up" => self.slash.step(-1, rows.len()),
            "down" => self.slash.step(1, rows.len()),
            "tab" | "enter" => {
                let at = self.slash.selected.min(rows.len() - 1);
                self.pick_slash(&rows[at], cx);
            }
            "escape" => {
                self.slash.hidden_at = Some(self.tools.editor.read(cx).text().to_owned());
            }
            _ => return,
        }
        cx.stop_propagation();
        self.redraw_chrome(cx);
    }
}

/// The list, its bottom on the composer's top, or nothing.
pub fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let rows = view.slash_rows(cx);
    if rows.is_empty() {
        return None;
    }
    let selected = view.slash.selected.min(rows.len() - 1);
    let mut list = card(a)
        .w(relative(1.0))
        .p(px(6.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .shadow_lg();
    for (i, command) in rows.into_iter().enumerate() {
        let hint = command.argument_hint.clone().unwrap_or_default();
        let pick = command.clone();
        let row = div()
            .id(SharedString::from(format!("slash-{}", command.name)))
            .w_full()
            .px(px(10.0))
            .py(px(5.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .cursor_pointer()
            .when(i == selected, |r| r.fill(Ink::Raised, a))
            .child(
                label(&format!("/{}", command.name), TypeRole::Label, Ink::Text, a).flex_shrink_0(),
            )
            .when(!hint.is_empty(), |r| {
                r.child(label(&hint, TypeRole::Meta, Ink::TextFaint, a).flex_shrink_0())
            })
            .child(
                label(&command.description, TypeRole::Meta, Ink::TextMuted, a)
                    .flex_1()
                    .min_w(px(0.0))
                    .elide(),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.pick_slash(&pick, cx)));
        list = list.child(row);
    }
    Some(
        div()
            .absolute()
            .bottom(relative(1.0))
            .left(px(SIDE_PADDING))
            .right(px(SIDE_PADDING))
            .pb(px(6.0))
            .child(list)
            .part_at("Command List", Corner::TopTrailing, a, cx),
    )
}
