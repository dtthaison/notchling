use gpui::{div, prelude::*, Context, Div, Stateful};
use notchling_engine::sessions::Agent;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{icon_button, menu_panel, MenuItem};
use notchling_kit::Icon;
use notchling_locale as t;

use super::cards::A;
use super::view::NotchView;
use crate::chat::session_reload::mode_of;

impl NotchView {
    pub(super) fn reload_button(
        &self,
        agent: &Agent,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        mode_of(agent)?;
        let kind = if self.reload_menu {
            ButtonKind::Secondary
        } else {
            ButtonKind::Ghost
        };
        Some(
            icon_button("notch-more", Icon::More, kind, PartState::Rest, A).on_click(cx.listener(
                |this, _, _, cx| {
                    this.reload_menu = !this.reload_menu;
                    cx.notify();
                },
            )),
        )
    }

    pub(super) fn reload_menu_panel(&self, agent: &Agent, cx: &mut Context<Self>) -> Option<Div> {
        if !self.reload_menu || mode_of(agent).is_none() {
            return None;
        }
        let id = agent.id.clone();
        Some(
            div().flex().justify_end().child(
                menu_panel(A).child(
                    MenuItem::new("notch-reload", t::chat::reload_item().to_owned())
                        .build(A)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.reload_menu = false;
                            if this.ns_window.is_some() {
                                crate::mac::shell::open_chat_reload(cx, &id);
                            }
                            cx.notify();
                        })),
                ),
            ),
        )
    }
}
