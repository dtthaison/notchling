use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_kit::labels::Corner;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, icon, label, KitPart};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::vault_cards::card_frame;
use super::ChatView;

pub(crate) fn visible(login_needed: bool, loading: bool, has_session: bool) -> bool {
    login_needed && !loading && has_session
}

pub(super) fn shown(view: &ChatView) -> bool {
    visible(
        view.conversation.login_needed,
        view.loading,
        view.selected_session().is_some(),
    )
}

pub(super) fn banner(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    if !shown(view) {
        return None;
    }
    let open = button(
        SharedString::from("login-show-terminal"),
        t::chat::outbox_show_terminal(),
        ButtonKind::Secondary,
        PartState::Rest,
        a,
    )
    .flex_shrink_0()
    .on_click(cx.listener(|this, _, _, cx| this.open_session_terminal(cx)));
    Some(
        card_frame(a)
            .flex()
            .items_start()
            .gap(px(8.0))
            .p(px(8.0))
            .child(icon(
                Icon::Warning,
                TypeRole::Label.spec().size,
                Ink::Needs,
                a,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap(px(2.0))
                    .child(label(
                        t::chat::login_needed_title(),
                        TypeRole::Label,
                        Ink::Needs,
                        a,
                    ))
                    .child(label(
                        t::chat::login_needed_body(),
                        TypeRole::Meta,
                        Ink::Text,
                        a,
                    )),
            )
            .child(open)
            .part_at("Login Needed", Corner::TopTrailing, a, cx),
    )
}
