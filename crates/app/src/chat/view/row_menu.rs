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
use notchling_locale as t;

use super::accounts::{check_item, held_item, menu_item};
use super::ChatView;
use crate::acting::{Control, Request};
use crate::chat::closed;
use crate::chat::data::Session;
use crate::chat::data::Tool;
use crate::chat::remote::{self, bring_back_job};
use crate::chat::remote_control::Action;
use crate::chat::sidebar::copyable_id;
use crate::chat::used;
use crate::phone::bridge::now_ms;

#[derive(Clone, Debug)]
pub struct RowMenu {
    pub row_id: String,
    pub at: Point<Pixels>,
    pub confirm_remove: bool,
}

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
            this.chrome_peek(&row_id, cx);
            cx.stop_propagation();
            cx.notify();
        }),
    )
}

pub(super) fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let menu = view.row_menu.as_ref()?;
    let id = copyable_id(&menu.row_id).map(str::to_owned);
    let session = view.sessions.iter().find(|s| s.agent.id == menu.row_id);
    let tool = session.map(|s| s.tool);
    let remote = session.filter(|s| remote::is_remote(&s.agent));
    let removable = session.is_some_and(|s| {
        s.tool == Tool::Claude
            && matches!(s.agent.target, AgentTarget::Job(_))
            && !remote::is_remote(&s.agent)
    });
    if removable && menu.confirm_remove {
        return Some(layer(menu, confirm_card(view, a, cx), cx));
    }
    let keep = view.cluster_snapshot().keep_offer(&menu.row_id);
    let mut items = if keep.is_some() {
        menu_panel_wide(a)
    } else {
        menu_panel(a)
    };
    items = match id {
        Some(id) => items.child(
            menu_item("row-copy-id", t::chat::copy_session_id(), a).on_click(cx.listener(
                move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(id.clone()));
                    this.row_menu = None;
                    cx.notify();
                },
            )),
        ),
        None => items.child(held_item(
            "row-copy-id",
            t::chat::copy_session_id(),
            Control {
                enabled: false,
                tooltip: Some(t::chat::session_not_started()),
            },
            a,
        )),
    };
    if let Some(tool) = tool.filter(|t| closed::closable(*t)) {
        let row = menu.row_id.clone();
        let all = match tool {
            Tool::Codex => t::chat::close_all_codex(),
            _ => t::chat::close_all_antigravity(),
        };
        items = items
            .child(
                menu_item("row-close", t::chat::close_session(), a).on_click(cx.listener(
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
    if let Some(on) = view.chrome_menu_item(&menu.row_id) {
        let row = menu.row_id.clone();
        let control = view.acting_control(cx);
        items = items.child(if control.enabled {
            check_item("row-chrome", t::chat::chrome_use(), on, a)
                .on_click(cx.listener(move |this, _, _, cx| this.chrome_toggle(&row, cx)))
        } else {
            held_item("row-chrome", t::chat::chrome_use(), control, a)
        });
    }
    if removable {
        let control = view.acting_control(cx);
        items = items.child(if control.enabled {
            menu_item("row-remove", t::chat::remove_session(), a).on_click(cx.listener(
                |this, _, _, cx| {
                    if let Some(menu) = this.row_menu.as_mut() {
                        menu.confirm_remove = true;
                    }
                    cx.notify();
                },
            ))
        } else {
            held_item("row-remove", t::chat::remove_session(), control, a)
        });
    }
    if let Some(offer) = keep {
        let row = menu.row_id.clone();
        items = items
            .child(menu_rule(a))
            .child(
                check_item(
                    "row-keep",
                    &t::cluster::keep_on(&offer.machine),
                    offer.kept,
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.keep_on_machine(&row, cx))),
            )
            .child(menu_note(t::cluster::keep_note(), a));
    }
    if let Some(session) = remote {
        let id = session.agent.id.clone();
        if let Some(control) = view.remote_control(&id, Action::Remove) {
            items = items.child(held_item(
                "row-remove",
                t::chat::remove_session(),
                control,
                a,
            ));
        }
        if bring_back_job(&session.agent).is_some() {
            let control = view.remote_control(&id, Action::BringBack);
            items = items.child(match control {
                Some(control) => held_item("row-bring-back", t::cluster::bring_back(), control, a),
                None => menu_item("row-bring-back", t::cluster::bring_back(), a)
                    .on_click(cx.listener(move |this, _, _, cx| this.bring_back(id.clone(), cx))),
            });
        }
    }
    Some(layer(menu, items, cx))
}

fn confirm_card(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let title = view
        .row_menu
        .as_ref()
        .and_then(|m| view.sessions.iter().find(|s| s.agent.id == m.row_id))
        .map(|s| s.agent.name.clone())
        .unwrap_or_default();
    menu_panel_wide(a)
        .child(
            label(&t::chat::remove_title(&title), TypeRole::Meta, Ink::Text, a)
                .px(px(MENU_ROW_X))
                .py(px(MENU_ROW_Y)),
        )
        .child(menu_note(t::chat::remove_note(), a))
        .child(menu_rule(a))
        .child(
            menu_item("row-remove-confirm", t::chat::remove(), a).on_click(cx.listener(
                |this, _, _, cx| {
                    if let Some(menu) = this.row_menu.take() {
                        this.remove_row(menu.row_id, cx);
                    }
                },
            )),
        )
        .child(
            menu_item("row-remove-cancel", t::chat::cancel(), a).on_click(cx.listener(
                |this, _, _, cx| {
                    this.row_menu = None;
                    cx.notify();
                },
            )),
        )
}

fn layer(menu: &RowMenu, items: Div, cx: &mut Context<ChatView>) -> Div {
    popup("row-menu-click-away", menu.at, items, close_row_menu, cx)
}

fn close_row_menu(this: &mut ChatView, cx: &mut Context<ChatView>) {
    this.row_menu = None;
    cx.notify();
}

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
    pub(super) fn load_closed(&mut self) {
        if let Some(store) = self.settings_store() {
            self.closed = closed::from_value(store.get(closed::KEY).as_ref());
            self.used = closed::from_value(store.get(used::KEY).as_ref());
        }
    }

    pub(super) fn mark_used(&mut self, id: &str) {
        if !used::mark(&mut self.used, id, now_ms() as f64 / 1000.0) {
            return;
        }
        self.save_used();
    }

    fn save_used(&mut self) {
        if let Some(mut store) = self.settings_store() {
            if let Err(e) = store.set(used::KEY, closed::to_value(&self.used)) {
                self.tools.error = Some(t::chat::save_used_failed(&e.to_string()));
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
                self.tools.error = Some(t::chat::save_closed_failed(&e.to_string()));
            }
        }
    }

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

    pub(super) fn reopen_row(&mut self, id: &str) {
        if self.closed.remove(id).is_some() {
            self.save_closed();
        }
    }

    fn acting_control(&self, cx: &mut Context<Self>) -> Control {
        self.acting(cx).control()
    }

    fn remove_row(&mut self, id: String, cx: &mut Context<Self>) {
        if self.tools.sending
            || self.remote_refused(&id, Action::Remove)
            || !self.acting_control(cx).enabled
        {
            cx.notify();
            return;
        }
        self.spawn_send(
            Request::remove(&id),
            move |view, result, cx| match result {
                Ok(_) => {
                    if view.selected.as_deref() == Some(id.as_str()) {
                        let folder_before = view.selected_folder();
                        view.selected = None;
                        view.branches_switched(folder_before, cx);
                    }
                    view.reload_now(cx);
                }
                Err(message) => view.tools.error = Some(message),
            },
            cx,
        );
    }

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
        let folder_before = self.selected_folder();
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
                None => {
                    self.selected = None;
                    self.branches_switched(folder_before, cx);
                }
            }
        }
        cx.notify();
    }
}
