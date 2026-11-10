//! The menu a right click on a sidebar row opens, at the pointer: Copy Session ID for now.

use notchling_engine::settings::Settings;

use gpui::{
    div, prelude::*, px, ClipboardItem, Context, Div, MouseButton, MouseDownEvent, Pixels, Point,
    SharedString, Stateful,
};
use notchling_kit::metric::RADIUS_MD;
use notchling_kit::ui::card;
use notchling_kit::Appearance;

use super::accounts::{held_item, menu_item};
use super::ChatView;
use crate::acting::Control;
use crate::chat::closed;
use crate::chat::data::Session;
use crate::chat::data::Tool;
use crate::chat::sidebar::copyable_id;
use crate::phone::bridge::now_ms;

/// An open row menu: the row it is for and where the click was, in the window.
#[derive(Clone, Debug)]
pub struct RowMenu {
    pub row_id: String,
    pub at: Point<Pixels>,
}

const WIDTH: f32 = 200.0;

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
    let tool = view
        .sessions
        .iter()
        .find(|s| s.agent.id == menu.row_id)
        .map(|s| s.tool);
    let mut items = card(a)
        .w(px(WIDTH))
        .p(px(6.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .shadow_lg();
    items = match id {
        Some(id) => items.child(
            menu_item(
                SharedString::from("row-copy-id"),
                "Copy Session ID",
                false,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(id.clone()));
                this.row_menu = None;
                cx.notify();
            })),
        ),
        // A draft has no id yet: the item shows, faint, and does nothing.
        None => items.child(held_item(
            SharedString::from("row-copy-id"),
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
                menu_item(SharedString::from("row-close"), "Close Session", false, a).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.close_rows(vec![row.clone()], cx);
                    }),
                ),
            )
            .child(
                menu_item(SharedString::from("row-close-all"), all, false, a).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let ids = closed::ids_of(&this.sessions, tool);
                        this.close_rows(ids, cx);
                    }),
                ),
            );
    }
    let close = |this: &mut ChatView, cx: &mut Context<ChatView>| {
        this.row_menu = None;
        cx.notify();
    };
    Some(
        div()
            .absolute()
            .size_full()
            .child(
                div()
                    .id("row-menu-click-away")
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
                    .left(menu.at.x)
                    .top(menu.at.y)
                    .occlude()
                    .rounded(px(RADIUS_MD))
                    .child(items),
            ),
    )
}

impl ChatView {
    /// Reads the closed rows from the settings file (none in a fixture or a render).
    pub(super) fn load_closed(&mut self) {
        if let Some(store) = self.settings_store() {
            self.closed = closed::from_value(store.get(closed::KEY).as_ref());
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

    /// The rows the sidebar shows: all but the closed Codex and Antigravity ones.
    pub(super) fn open_rows(&self, all: Vec<Session>) -> Vec<Session> {
        all.into_iter()
            .filter(|s| !closed::is_closed(s, &self.closed))
            .collect()
    }

    /// A closed row opened again (from Search).
    pub(super) fn reopen_row(&mut self, id: &str) {
        if self.closed.remove(id).is_some() {
            self.save_closed();
        }
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
