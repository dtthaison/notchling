//! The menu a right click on a sidebar row opens, at the pointer: Copy Session ID, Remove Session
//! for a Claude Code background session, Close for Codex and Antigravity rows.

use notchling_engine::sessions::AgentTarget;
use notchling_engine::settings::Settings;

use gpui::{
    div, prelude::*, px, ClipboardItem, Context, Div, MouseButton, MouseDownEvent, Pixels, Point,
    Stateful,
};
use notchling_kit::metric::MENU_RADIUS;
use notchling_kit::metric::{MENU_ROW_X, MENU_ROW_Y};
use notchling_kit::ui::{label, menu_note, menu_panel, menu_panel_wide, menu_rule};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::accounts::{held_item, menu_item};
use super::ChatView;
use crate::acting::{Control, Request};
use crate::chat::closed;
use crate::chat::data::Session;
use crate::chat::data::Tool;
use crate::chat::sidebar::copyable_id;
use crate::chat::used;
use crate::phone::bridge::now_ms;

/// An open row menu: the row it is for and where the click was, in the window.
#[derive(Clone, Debug)]
pub struct RowMenu {
    pub row_id: String,
    pub at: Point<Pixels>,
    /// Remove Session… was clicked: the menu asks once more before removing.
    pub confirm_remove: bool,
}

/// Opens the menu on a right click on `row_id`'s row.
pub(super) fn on_right_click(
    row: Stateful<Div>,
    row_id: String,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    row.on_mouse_down(
        MouseButton::Right,
        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
            this.row_menu = Some(RowMenu {
                row_id: row_id.clone(),
                at: event.position,
                confirm_remove: false,
            });
            cx.stop_propagation();
            cx.notify();
        }),
    )
}

/// The open menu over a click-away layer, or nothing.
pub(super) fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let menu = view.row_menu.as_ref()?;
    let id = copyable_id(&menu.row_id).map(str::to_owned);
    let session = view.sessions.iter().find(|s| s.agent.id == menu.row_id);
    let tool = session.map(|s| s.tool);
    // `claude rm` removes a background job; a terminal session belongs to its terminal.
    let removable = session
        .is_some_and(|s| s.tool == Tool::Claude && matches!(s.agent.target, AgentTarget::Job(_)));
    if removable && menu.confirm_remove {
        return Some(layer(menu, confirm_card(view, a, cx), cx));
    }
    let mut items = menu_panel(a);
    items = match id {
        Some(id) => items.child(menu_item("row-copy-id", "Copy Session ID", a).on_click(
            cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(id.clone()));
                this.row_menu = None;
                cx.notify();
            }),
        )),
        // A draft has no id yet: the item shows, faint, and does nothing.
        None => items.child(held_item(
            "row-copy-id",
            "Copy Session ID",
            Control {
                enabled: false,
                tooltip: Some("This session hasn't started yet, so it has no id."),
            },
            a,
        )),
    };
    // Codex and Antigravity rows close: off the sidebar until they're used again.
    if let Some(tool) = tool.filter(|t| closed::closable(*t)) {
        let row = menu.row_id.clone();
        let all = match tool {
            Tool::Codex => "Close All Codex Sessions",
            _ => "Close All Antigravity Sessions",
        };
        items = items
            .child(
                menu_item("row-close", "Close Session", a).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.close_rows(vec![row.clone()], cx);
                    },
                )),
            )
            .child(menu_item("row-close-all", all, a).on_click(cx.listener(
                move |this, _, _, cx| {
                    let ids = closed::ids_of(&this.sessions, tool);
                    this.close_rows(ids, cx);
                },
            )));
    }
    if removable {
        let control = view.acting_control(cx);
        items = items.child(if control.enabled {
            menu_item("row-remove", "Remove Session…", a).on_click(cx.listener(|this, _, _, cx| {
                if let Some(menu) = this.row_menu.as_mut() {
                    menu.confirm_remove = true;
                }
                cx.notify();
            }))
        } else {
            held_item("row-remove", "Remove Session…", control, a)
        });
    }
    Some(layer(menu, items, cx))
}

/// The second step of Remove Session…: what it does, then Remove or Cancel.
fn confirm_card(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let title = view
        .row_menu
        .as_ref()
        .and_then(|m| view.sessions.iter().find(|s| s.agent.id == m.row_id))
        .map(|s| s.agent.name.clone())
        .unwrap_or_default();
    menu_panel_wide(a)
        .child(
            label(&format!("Remove “{title}”?"), TypeRole::Meta, Ink::Text, a)
                .px(px(MENU_ROW_X))
                .py(px(MENU_ROW_Y)),
        )
        .child(menu_note(
            "Claude Code stops it and removes it from its list. This can't be undone.",
            a,
        ))
        .child(menu_rule(a))
        .child(
            menu_item("row-remove-confirm", "Remove", a).on_click(cx.listener(|this, _, _, cx| {
                if let Some(menu) = this.row_menu.take() {
                    this.remove_row(menu.row_id, cx);
                }
            })),
        )
        .child(
            menu_item("row-remove-cancel", "Cancel", a).on_click(cx.listener(|this, _, _, cx| {
                this.row_menu = None;
                cx.notify();
            })),
        )
}

/// The menu at the click, over a layer that closes it on a click anywhere else.
fn layer(menu: &RowMenu, items: Div, cx: &mut Context<ChatView>) -> Div {
    popup("row-menu-click-away", menu.at, items, close_row_menu, cx)
}

fn close_row_menu(this: &mut ChatView, cx: &mut Context<ChatView>) {
    this.row_menu = None;
    cx.notify();
}

/// `items` at `at` over a layer that runs `close` on a press anywhere else, either button. The
/// right-click menus share it (this one, `copy_menu`).
pub(super) fn popup(
    id: &'static str,
    at: Point<Pixels>,
    items: Div,
    close: fn(&mut ChatView, &mut Context<ChatView>),
    cx: &mut Context<ChatView>,
) -> Div {
    div()
        .absolute()
        .size_full()
        .child(
            div()
                .id(id)
                .absolute()
                .size_full()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| close(this, cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, _, cx| close(this, cx)),
                ),
        )
        .child(
            div()
                .absolute()
                .left(at.x)
                .top(at.y)
                .occlude()
                .rounded(px(MENU_RADIUS))
                .child(items),
        )
}

impl ChatView {
    /// Reads the closed rows from the settings file (none in a fixture or a render).
    pub(super) fn load_closed(&mut self) {
        if let Some(store) = self.settings_store() {
            self.closed = closed::from_value(store.get(closed::KEY).as_ref());
            self.used = closed::from_value(store.get(used::KEY).as_ref());
        }
    }

    /// Records that `id` was used here, so the Sidebar keeps showing it.
    pub(super) fn mark_used(&mut self, id: &str) {
        if !used::mark(&mut self.used, id, now_ms() as f64 / 1000.0) {
            return;
        }
        self.save_used();
    }

    fn save_used(&mut self) {
        if let Some(mut store) = self.settings_store() {
            if let Err(e) = store.set(used::KEY, closed::to_value(&self.used)) {
                self.tools.error = Some(format!("Couldn't save the sessions used in Chat: {e}"));
            }
        }
    }

    fn settings_store(&self) -> Option<Settings> {
        if self.root.fixture.is_some() {
            return None;
        }
        Settings::open(&self.root.data_dir.join("settings.json")).ok()
    }

    fn save_closed(&mut self) {
        if let Some(mut store) = self.settings_store() {
            if let Err(e) = store.set(closed::KEY, closed::to_value(&self.closed)) {
                self.tools.error = Some(format!("Couldn't save the closed sessions: {e}"));
            }
        }
    }

    /// The rows the sidebar shows: Claude Code's, and the Codex and Antigravity ones used here or
    /// working or recent (`chat::used`; the selected one too) that weren't closed since. A fixture or a render, with
    /// no settings to remember uses in, shows every row.
    pub(super) fn open_rows(&self, all: Vec<Session>) -> Vec<Session> {
        let remembers = self.root.fixture.is_none();
        let now_s = now_ms() as f64 / 1000.0;
        all.into_iter()
            .filter(|s| !used::is_delegated_job(&s.agent.id, &self.delegated, &self.used))
            .filter(|s| {
                !remembers
                    || used::shown(s, &self.used, now_s)
                    || self.selected.as_deref() == Some(s.agent.id.as_str())
            })
            .filter(|s| !closed::is_closed(s, &self.closed))
            .collect()
    }

    /// A closed row opened again (from Search).
    pub(super) fn reopen_row(&mut self, id: &str) {
        if self.closed.remove(id).is_some() {
            self.save_closed();
        }
    }

    /// Whether session actions may run now (the daemon is up and acting), with why not.
    fn acting_control(&self, cx: &mut Context<Self>) -> Control {
        self.acting(cx).control()
    }

    /// Removes a Claude Code background session (`session.remove`, `claude rm` in every folder
    /// holding a copy). The row goes when the daemon's list drops it; a failure shows as the error.
    fn remove_row(&mut self, id: String, cx: &mut Context<Self>) {
        if self.tools.sending || !self.acting_control(cx).enabled {
            cx.notify();
            return;
        }
        self.spawn_send(
            Request::remove(&id),
            move |view, result, cx| match result {
                Ok(_) => {
                    if view.selected.as_deref() == Some(id.as_str()) {
                        view.selected = None;
                    }
                    view.reload_now(cx);
                }
                Err(message) => view.tools.error = Some(message),
            },
            cx,
        );
    }

    /// Closes `ids`: off the sidebar now and after a restart; the selection moves to the first row
    /// left when it was one of them.
    fn close_rows(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        self.row_menu = None;
        if ids.is_empty() {
            cx.notify();
            return;
        }
        closed::close(&mut self.closed, &ids, now_ms() as f64 / 1000.0);
        self.save_closed();
        if used::forget(&mut self.used, &ids) {
            self.save_used();
        }
        let all = std::mem::take(&mut self.sessions);
        self.sessions = self.open_rows(all);
        let gone = self.selected.as_ref().is_some_and(|sel| ids.contains(sel));
        if gone {
            let next = crate::chat::sidebar::sections(&self.sessions, self.now_ms)
                .into_iter()
                .flat_map(|s| s.rows)
                .map(|r| r.id)
                .next();
            match next {
                Some(id) => self.select(&id, cx),
                None => self.selected = None,
            }
        }
        cx.notify();
    }
}
