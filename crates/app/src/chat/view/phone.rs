use std::sync::Arc;

use gpui::{div, prelude::*, px, Context, Div, Entity};
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::ui::{hsla, KitStyled};
use notchling_kit::{Appearance, Ink};
use notchling_locale as t;

use super::ChatView;
use crate::acting::Control;
use crate::mac::shell::AppBridge;
use crate::phone::bridge::{Bridge, Snapshot};
use crate::phone::view::{PhonePanelView, WIDTH};

const HEIGHT: f32 = 600.0;
const TOP: f32 = 60.0;

pub fn button_tip_on() -> Control {
    Control {
        enabled: true,
        tooltip: Some(t::chat::phone_tip_on()),
    }
}

pub fn button_tip_off() -> Control {
    Control {
        enabled: true,
        tooltip: Some(t::chat::phone_tip_off()),
    }
}

pub fn app_bridge(cx: &Context<ChatView>) -> Arc<Bridge> {
    cx.try_global::<AppBridge>()
        .map(|b| b.0.clone())
        .unwrap_or_else(|| Arc::new(Bridge::fixed(Snapshot::detached())))
}

impl ChatView {
    pub(super) fn bridge_on(&self) -> bool {
        let s = self.bridge.snapshot();
        s.attached && s.state.is_on()
    }

    pub(super) fn toggle_phone(&mut self, cx: &mut Context<Self>) {
        if self.phone.take().is_none() {
            let bridge = self.bridge.clone();
            self.phone = Some(cx.new(|cx| PhonePanelView::live(bridge, cx)));
        }
        cx.notify();
    }

    fn close_phone(&mut self, cx: &mut Context<Self>) {
        self.phone = None;
        cx.notify();
    }
}

pub fn render(
    phone: Option<&Entity<PhonePanelView>>,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Option<Div> {
    let panel = phone?;
    Some(
        div()
            .absolute()
            .size_full()
            .child(
                div()
                    .id("phone-click-away")
                    .absolute()
                    .size_full()
                    .occlude()
                    .on_click(cx.listener(|this, _, _, cx| this.close_phone(cx))),
            )
            .child(
                div()
                    .absolute()
                    .top(px(TOP))
                    .right(px(SIDE_PADDING))
                    .w(px(WIDTH))
                    .h(px(HEIGHT))
                    .occlude()
                    .rounded(px(14.0))
                    .shadow_lg()
                    .border(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
                    .fill(Ink::Window, a)
                    .child(panel.clone()),
            ),
    )
}
