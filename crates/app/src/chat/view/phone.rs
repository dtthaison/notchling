//! The Phone button's panel (part U6): a card under the header's button, drawn over the window
//! until a click lands outside it. The panel is created when it opens and dropped when it closes,
//! so its refresh loop runs only while it is on screen; the app-wide poll (`mac::shell`) keeps the
//! bridge fresh meanwhile, which is what the button's green dot reads.

use std::sync::Arc;

use gpui::{div, prelude::*, px, Context, Div, Entity};
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::ui::{hsla, KitStyled};
use notchling_kit::{Appearance, Ink};

use super::ChatView;
use crate::acting::Control;
use crate::mac::shell::AppBridge;
use crate::phone::bridge::{Bridge, Snapshot};
use crate::phone::view::{PhonePanelView, WIDTH};

/// The panel's height, short of the window's.
const HEIGHT: f32 = 600.0;
/// Under the header (56 high).
const TOP: f32 = 60.0;

/// What the button says on hover.
pub const BUTTON_TIP_ON: Control = Control {
    enabled: true,
    tooltip: Some("Phone: the bridge is on. Pair or manage phones"),
};
pub const BUTTON_TIP_OFF: Control = Control {
    enabled: true,
    tooltip: Some("Phone: pair a phone with this Mac"),
};

/// The app's bridge, or one with no daemon behind it (a render, a fixture root).
pub fn app_bridge(cx: &Context<ChatView>) -> Arc<Bridge> {
    cx.try_global::<AppBridge>()
        .map(|b| b.0.clone())
        .unwrap_or_else(|| Arc::new(Bridge::fixed(Snapshot::detached())))
}

impl ChatView {
    /// Whether the green dot shows on the Phone button: the daemon says the bridge is on.
    pub(super) fn bridge_on(&self) -> bool {
        let s = self.bridge.snapshot();
        s.attached && s.state.is_on()
    }

    /// The Phone button: opens the panel, or closes it.
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

/// The panel over a click-away layer, under the header's Phone button.
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
