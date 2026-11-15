use gpui::{div, prelude::*, px, App, Context, Div};
use notchling_engine::sessions::AgentGroup;
use notchling_kit::labels::Corner;
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, label, KitPart, KitStyled};
use notchling_kit::{sentence, Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::{AccountButton, ChatTool};
use crate::acting::Request;
use crate::chat::data::Session;
use crate::chat::pickers::Menu;
use crate::chat::view::limit_ring::Shown;
use crate::chat::view::ChatView;

pub(super) struct ShellJob;

impl ChatTool for ShellJob {
    fn placeholder(&self) -> &'static str {
        t::cluster::shell_running()
    }

    fn refusal(&self, _view: &ChatView, _session: &Session) -> Option<&'static str> {
        Some(t::cluster::shell_no_reply())
    }

    fn working(&self, session: &Session) -> bool {
        session.agent.group == AgentGroup::Working
    }

    fn send(&self, _view: &ChatView, session: &Session, text: &str) -> Request {
        Request::reply(&session.agent.id, text)
    }

    fn stop(&self, id: &str) -> Request {
        Request::shell_stop(id)
    }

    fn menus(&self, _view: &ChatView, _session: &Session, _cx: &App) -> Vec<Menu> {
        Vec::new()
    }

    fn pick(&self, _view: &mut ChatView, _menu: &str, _value: &str, _cx: &mut Context<ChatView>) {}

    fn account(&self, _view: &ChatView) -> Option<AccountButton> {
        None
    }

    fn account_click(&self, _view: &mut ChatView, _cx: &mut Context<ChatView>) {}

    fn limits(&self, _view: &ChatView, _session: &Session) -> Option<Shown> {
        None
    }
}

pub(in crate::chat::view) fn bar(
    view: &ChatView,
    session: &Session,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let running = ShellJob.working(session);
    let stopping = view.stopping();
    let status = if let Some(note) = view.offline_note() {
        note
    } else if let Some(error) = &view.tools.error {
        sentence(error)
    } else if stopping {
        t::cluster::shell_stopping().to_owned()
    } else if running {
        t::cluster::shell_running().to_owned()
    } else {
        t::cluster::shell_ended().to_owned()
    };
    let ink = if view.tools.error.is_some() {
        Ink::Needs
    } else {
        Ink::TextMuted
    };
    let live = running
        && !stopping
        && !view.tools.sending
        && !crate::chat::remote_control::refused(
            view.remote_control(&session.agent.id, crate::chat::remote_control::Action::Stop),
        );
    let stop = button(
        "shell-stop",
        t::cluster::shell_stop(),
        ButtonKind::Secondary,
        if live {
            PartState::Rest
        } else {
            PartState::Disabled
        },
        a,
    );
    let stop = if live {
        stop.on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
    } else {
        stop
    };
    div()
        .relative()
        .flex_shrink_0()
        .mx(px(SIDE_PADDING))
        .pt(px(8.0))
        .pb(px(12.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .child(label(&status, TypeRole::Meta, ink, a).elide()),
        )
        .child(stop.part_at("Stop Button", Corner::Top, a, cx))
        .part_at("Shell Job Bar", Corner::Trailing, a, cx)
}
