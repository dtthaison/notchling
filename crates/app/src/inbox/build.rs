use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyView, Context, Entity, Render, Window};
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::ui::{window_appearance, KitStyled};
use notchling_kit::Ink;

use super::feed::InboxFeed;
use super::renders::{consent_model, model, shown, NOW_MS};
use super::view::InboxPanelView;

struct InboxRender {
    panel: Entity<InboxPanelView>,
}

impl Render for InboxRender {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        div()
            .size_full()
            .p(px(SIDE_PADDING))
            .fill(Ink::Window, a)
            .child(self.panel.clone())
    }
}

pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    let done_open = shown(name)?;
    let consent = name.starts_with("inbox-consent");
    let fixture = match name {
        "inbox-consent" => consent_model(0),
        "inbox-consent-switch-acting" => consent_model(2),
        "inbox-consent-switch-vault" => consent_model(3),
        "inbox-consent-session" => consent_model(4),
        "inbox-consent-session-start" => consent_model(5),
        "inbox-consent-dispatch" => consent_model(6),
        "inbox-consent-cluster-invite" => consent_model(7),
        "inbox-consent-cluster-allow" => consent_model(8),
        "inbox-consent-account-switch" => consent_model(9),
        "inbox-consent-account-add" => consent_model(10),
        "inbox-consent-account-move-all" => consent_model(11),
        "inbox-consent-lane-switch" => consent_model(12),
        other if super::renders_p8::has(other) => super::renders_p8::model(other),
        _ if consent => consent_model(1),
        _ => model(),
    };
    let feed = Arc::new(InboxFeed::fixed(fixture));
    let vault_off = name == "inbox-consent-vault-off";
    let panel = cx.new(|_| {
        let panel = InboxPanelView::fixed(feed, NOW_MS, done_open);
        if vault_off {
            panel.with_vault(false)
        } else {
            panel
        }
    });
    Some(cx.new(|_| InboxRender { panel }).into())
}
