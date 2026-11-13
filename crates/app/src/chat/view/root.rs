use super::*;
use crate::mac::dock::Copy as CopyAction;

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        super::pane::ROOT_RENDERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let a = window_appearance(window);
        super::render_map::sync(self.chrome.entity_id(), cx);
        self.take_terminal_focus(window);
        self.keys.hold(window, cx);
        if let Some(text) = self.slash_preview.clone() {
            if self.selected_session().is_some() && self.tools.editor.read(cx).text() != text {
                self.tools
                    .editor
                    .update(cx, |editor, cx| editor.set_text(&text, cx));
                self.slash_typed(&text);
                self.redraw_chrome(cx);
            }
        }
        self.sync_prompt(cx);
        let inset = super::tasks::shown(self);
        self.sync_tasks(cx);
        if super::tasks::shown(self) != inset {
            self.refresh_conversation(cx);
        }
        self.overlay_open = self.sheet.is_some()
            || self.account_menu
            || self.row_menu.is_some()
            || self.copy_menu.is_some()
            || self.phone.is_some()
            || self.inbox_panel.is_some()
            || self.tools.picker.is_some()
            || self.composer.is_open()
            || parts_on(cx);
        self.activity_drawn.set(None);
        self.parts.root_spins.clear();
        self.parts.root_spins.seen();
        self.parts
            .native
            .set(!native_spin::covered(native_spin::Covers {
                sheet: self.sheet.is_some(),
                menu: self.account_menu || self.row_menu.is_some() || self.phone.is_some(),
                composer_menu: self.tools.picker.is_some()
                    || self.composer.is_open()
                    || !self.slash_rows(cx).is_empty(),
                limit_card: self.limit_ring.card_open(),
                drop_hint: self.drop_preview,
                part_names: parts_on(cx),
                inbox: self.inbox_panel.is_some(),
            }));
        self.page_covered =
            crate::chat::page_panel::page_covered(crate::chat::page_panel::Covers {
                sheet: self.sheet.is_some(),
                phone: self.phone.is_some(),
                inbox: self.inbox_panel.is_some(),
                parts: parts_on(cx),
            });
        let in_subagent = self.open_subagent.is_some();
        let takes_drops = !in_subagent && self.composer_control(cx).enabled;
        let conversation = div()
            .relative()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .child(self.parts.conversation(cx))
            .children(tasks::render(self, a, cx))
            .children(super::render_map::overlay(
                notchling_kit::render_map::Region::Conversation,
                super::render_map::Place::Bottom(12.0),
                a,
                cx,
            ))
            .part_at(
                if in_subagent {
                    "Subagent View"
                } else {
                    "Conversation"
                },
                Corner::Bottom,
                a,
                cx,
            );
        let terminal_expanded =
            self.terminal.shown() && self.terminal.expanded && !self.page.is_open();
        let column = div()
            .id("chat-column")
            .group(drop::GROUP)
            .relative()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .flex()
            .flex_col()
            .children(subagent::header(self, a, cx))
            .child(conversation)
            .children((!in_subagent).then(|| composer::render(self, a, window, cx)))
            .when(takes_drops, |column| {
                column.on_drop::<ExternalPaths>(cx.listener(
                    |this, paths: &ExternalPaths, w, cx| {
                        this.drop_paths(paths.paths(), w, cx);
                    },
                ))
            })
            .when(takes_drops || self.drop_preview, |column| {
                column.child(drop::hint(self.drop_preview, a))
            });
        div()
            .relative()
            .size_full()
            .flex()
            .fill(Ink::Window, a)
            .capture_any_mouse_down(cx.listener(|this, _, _, _| this.page.take_keys_back()))
            .on_action(cx.listener(|this, _: &CopyAction, window, cx| {
                if !this.copy_selection(window, cx) {
                    cx.propagate();
                }
            }))
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                this.capture_slash_key(event, window, cx)
            }))
            .child(self.keys.anchor())
            .children((!self.sidebar_hidden).then(|| self.parts.sidebar(a, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(header::render(self, a, cx))
                    .child(page_panel::follows_edge(
                        div()
                            .flex_1()
                            .min_h(px(0.0))
                            .flex()
                            .when(!terminal_expanded, |row| row.child(column))
                            .children(page_panel::render(self, a, window, cx))
                            .children(changes::render(self, a, window, cx))
                            .children(terminal::render(self, a, window, cx))
                            .children(
                                (!self.terminal.shown())
                                    .then(|| simulator::render(self, a, window, cx))
                                    .flatten(),
                            )
                            .children(
                                (!self.page.is_open()
                                    && !self.repo.changes_shown
                                    && !self.terminal.shown()
                                    && !self.simulator.shown()
                                    && !self.details_hidden)
                                    .then(|| self.parts.details(a, cx)),
                            ),
                        cx,
                    )),
            )
            .child(self.parts.spins.clone())
            .child(sidebar::toggle_button(self, a, cx))
            .child(sidebar::new_session_button(self, a, cx))
            .children(accounts::render_menu(self, a, cx))
            .children(row_menu::render(self, a, cx))
            .children(copy_menu::render(self, a, cx))
            .children(sheets::render(self, a))
            .children(phone::render(self.phone.as_ref(), a, cx))
            .children(inbox::render(self.inbox_panel.as_ref(), cx))
            .child(resize_edges(window, TopBorder::InContent))
            .child(self.chrome.clone())
    }
}
