//! The window's root draw: the three parts, the panels, the header, the reply box and every
//! overlay.

use super::*;
use crate::mac::dock::Copy as CopyAction;

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        super::pane::ROOT_RENDERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let a = window_appearance(window);
        super::render_map::sync(self.chrome.entity_id(), cx);
        self.take_terminal_focus(window);
        // With no focus a key reaches only the window's outermost node: keep Copy reachable.
        self.keys.hold(window, cx);
        if let Some(text) = self.slash_preview.clone() {
            if self.selected_session().is_some() && self.tools.editor.read(cx).text() != text {
                self.tools
                    .editor
                    .update(cx, |editor, cx| editor.set_text(&text, cx));
                self.slash_typed(&text);
            }
        }
        self.sync_prompt(cx);
        // The Tasks bar's room is the conversation's input, and this is where the bar's state is
        // taken: a change in it is drawn in the same frame.
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
            || self.tools.picker.is_some()
            || self.composer.is_open()
            || parts_on(cx);
        // Set again when the composer draws the Activity Line below.
        self.activity_drawn.set(None);
        // The Activity Line's spinner is laid out below: this draw's slot replaces last draw's.
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
            }));
        self.page_covered =
            crate::chat::page_panel::page_covered(crate::chat::page_panel::Covers {
                sheet: self.sheet.is_some(),
                phone: self.phone.is_some(),
                parts: parts_on(cx),
            });
        // Files from Finder land anywhere on the middle column while the box can take them.
        let in_subagent = self.open_subagent.is_some();
        let takes_drops = !in_subagent && self.composer_control(cx).enabled;
        let conversation = div()
            .relative()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .child(self.parts.conversation(cx))
            // Over the messages, so it stays while they scroll.
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
            // A subagent's transcript is read-only: no reply box under it.
            .children((!in_subagent).then(|| composer::render(self, a, window, cx)))
            .when(takes_drops, |column| {
                column.on_drop::<ExternalPaths>(cx.listener(
                    |this, paths: &ExternalPaths, w, cx| {
                        this.drop_paths(paths.paths(), w, cx);
                    },
                ))
            })
            // The hint only where a drop would land; a render over a fixture has no acting, so
            // its switch draws the hint anyway.
            .when(takes_drops || self.drop_preview, |column| {
                column.child(drop::hint(self.drop_preview, a))
            });
        div()
            .relative()
            .size_full()
            .flex()
            .fill(Ink::Window, a)
            // A click in the page panel gives its web view the keyboard; a click anywhere GPUI
            // draws takes it back, or typing would go on reaching the page.
            .capture_any_mouse_down(cx.listener(|this, _, _, _| this.page.take_keys_back()))
            // Copy (⌘C, the Edit menu, the right-click menu): the selection in the conversation
            // (`row_select`), the file panel (`select`) or the reply box. A press anywhere else
            // lets the first two go, so the reply box's own ⌘C isn't taken. Without a selection
            // the action goes on to the key. Then the command list's keys, before the reply box
            // moves its caret on Up and Down.
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
            // The header runs across the conversation and the Details Panel, so both start under it.
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(header::render(self, a, cx))
                    // A page opened from a link takes the Details Panel's place.
                    .child(page_panel::follows_edge(
                        div()
                            .flex_1()
                            .min_h(px(0.0))
                            .flex()
                            // An expanded terminal takes the conversation's room too.
                            .when(!terminal_expanded, |row| row.child(column))
                            .children(page_panel::render(self, a, window, cx))
                            .children(terminal::render(self, a, window, cx))
                            .children(
                                (!self.terminal.shown())
                                    .then(|| simulator::render(self, a, window, cx))
                                    .flatten(),
                            )
                            .children(
                                (!self.page.is_open()
                                    && !self.terminal.shown()
                                    && !self.simulator.shown()
                                    && !self.details_hidden)
                                    .then(|| self.parts.details(a, cx)),
                            ),
                        cx,
                    )),
            )
            // The spinners of the parts above, over them and under what follows.
            .child(self.parts.spins.clone())
            // Beside the traffic lights, over the sidebar's top or the header's start.
            .child(sidebar::toggle_button(self, a, cx))
            .child(sidebar::new_session_button(self, a, cx))
            .children(accounts::render_menu(self, a, cx))
            .children(row_menu::render(self, a, cx))
            .children(copy_menu::render(self, a, cx))
            .children(sheets::render(self, a))
            .children(phone::render(self.phone.as_ref(), a, cx))
            .child(resize_edges(window, TopBorder::InContent))
            .child(self.chrome.clone())
    }
}
