use std::sync::Arc;
use std::time::Duration;

use gpui::{
    prelude::*, px, size, App, Bounds, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions,
};
use objc2_app_kit::{NSFloatingWindowLevel, NSWindowCollectionBehavior};

use super::capture::ns_window;
use crate::phone::approval::PROMPT_SIZE;
use crate::phone::bridge::{now_ms, Bridge};
use crate::phone::pairing::PairingFeed;
use crate::phone::prompt_view::PairPromptView;

const TICK: Duration = Duration::from_millis(200);

pub fn watch(cx: &mut App, feed: Arc<PairingFeed>, bridge: Arc<Bridge>) {
    cx.spawn(async move |cx| {
        let mut window: Option<WindowHandle<PairPromptView>> = None;
        let mut seen = feed.bridge_changes();
        loop {
            cx.background_executor().timer(TICK).await;
            let changes = feed.bridge_changes();
            if changes != seen {
                seen = changes;
                let asked = bridge.clone();
                cx.background_executor()
                    .spawn(async move { asked.refresh() })
                    .detach();
            }
            let waiting = feed.prompt(now_ms()).is_some();
            let (feed, bridge) = (feed.clone(), bridge.clone());
            let gone = window.take();
            let next = cx.update(move |cx| sync(cx, gone, waiting, feed, bridge));
            match next {
                Ok(next) => window = next,
                Err(_) => return,
            }
        }
    })
    .detach();
}

fn sync(
    cx: &mut App,
    window: Option<WindowHandle<PairPromptView>>,
    waiting: bool,
    feed: Arc<PairingFeed>,
    bridge: Arc<Bridge>,
) -> Option<WindowHandle<PairPromptView>> {
    match (window, waiting) {
        (Some(handle), false) => {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
            None
        }
        (Some(handle), true) => handle.update(cx, |_, _, _| ()).ok().map(|()| handle),
        (None, true) => open(cx, feed, bridge),
        (None, false) => None,
    }
}

fn open(
    cx: &mut App,
    feed: Arc<PairingFeed>,
    bridge: Arc<Bridge>,
) -> Option<WindowHandle<PairPromptView>> {
    let bounds = Bounds::centered(None, size(px(PROMPT_SIZE.0), px(PROMPT_SIZE.1)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        window_background: WindowBackgroundAppearance::Opaque,
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };
    let opened = cx
        .open_window(options, |window, cx| {
            if let Some(w) = ns_window(window) {
                w.setLevel(NSFloatingWindowLevel);
                w.setCollectionBehavior(
                    NSWindowCollectionBehavior::CanJoinAllSpaces
                        | NSWindowCollectionBehavior::FullScreenAuxiliary
                        | NSWindowCollectionBehavior::IgnoresCycle,
                );
                w.setHidesOnDeactivate(false);
            }
            cx.new(|cx| PairPromptView::live(feed, bridge, cx))
        })
        .map_err(|e| eprintln!("open the pairing prompt: {e}"))
        .ok();
    if opened.is_some() {
        cx.activate(true);
    }
    opened
}
