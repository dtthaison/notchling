//! The window for the restart notice: while the daemon has switches off that this app saw on (or
//! the person chose on), a small card near the top of the screen says so with Turn Back On. It
//! never takes the focus or brings the app forward: nothing waits on it, and a key pressed in
//! another app must not land in it. Closing it is Not Now.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    prelude::*, px, size, App, Bounds, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions,
};

use crate::switches::restart::{RestartNotice, TurnOn};
use crate::switches::restart_view::{RestartNoticeView, NOTICE_SIZE};

/// How often the notice is looked at.
const TICK: Duration = Duration::from_secs(1);

/// Starts the loop that opens the notice window when something is offered and closes it when
/// nothing is.
pub fn watch(cx: &mut App, notice: Arc<RestartNotice>, turn_on: Arc<dyn TurnOn>) {
    cx.spawn(async move |cx| {
        let mut window: Option<WindowHandle<RestartNoticeView>> = None;
        loop {
            cx.background_executor().timer(TICK).await;
            let offered = !notice.state().offer().is_empty();
            let (n, t) = (notice.clone(), turn_on.clone());
            let gone = window.take();
            match cx.update(move |cx| sync(cx, gone, offered, n, t)) {
                Ok(next) => window = next,
                // The app is quitting.
                Err(_) => return,
            }
        }
    })
    .detach();
}

fn sync(
    cx: &mut App,
    window: Option<WindowHandle<RestartNoticeView>>,
    offered: bool,
    notice: Arc<RestartNotice>,
    turn_on: Arc<dyn TurnOn>,
) -> Option<WindowHandle<RestartNoticeView>> {
    match (window, offered) {
        (Some(handle), false) => {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
            None
        }
        (Some(handle), true) => match handle.update(cx, |_, _, _| ()) {
            Ok(()) => Some(handle),
            // The person closed it: Not Now.
            Err(_) => {
                notice.dismiss();
                None
            }
        },
        (None, true) => {
            let bounds = Bounds::centered(None, size(px(NOTICE_SIZE.0), px(NOTICE_SIZE.1)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: None,
                window_background: WindowBackgroundAppearance::Opaque,
                focus: false,
                show: true,
                kind: WindowKind::PopUp,
                is_resizable: false,
                is_minimizable: false,
                ..Default::default()
            };
            cx.open_window(options, |_, cx| {
                cx.new(|_| RestartNoticeView::live(notice, turn_on))
            })
            .map_err(|e| eprintln!("open the restart notice: {e}"))
            .ok()
        }
        (None, false) => None,
    }
}
