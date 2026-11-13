use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    prelude::*, px, size, App, Bounds, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions,
};

use crate::switches::restart::{restore, Area, RestartNotice, TurnOn};
use crate::switches::restart_view::{window_size, RestartNoticeView};

const TICK: Duration = Duration::from_secs(1);

pub fn watch(cx: &mut App, notice: Arc<RestartNotice>, turn_on: Arc<dyn TurnOn>) {
    cx.spawn(async move |cx| {
        let mut window: Option<WindowHandle<RestartNoticeView>> = None;
        let mut tried: BTreeSet<Area> = BTreeSet::new();
        loop {
            cx.background_executor().timer(TICK).await;
            let offer = notice.state().offer();
            tried.retain(|a| offer.contains(a));
            let fresh: Vec<Area> = offer
                .iter()
                .filter(|a| !tried.contains(a))
                .copied()
                .collect();
            if !fresh.is_empty() {
                tried.extend(fresh.iter().copied());
                let (n, t) = (notice.clone(), turn_on.clone());
                let failed = cx
                    .background_executor()
                    .spawn(async move { restore(&fresh, t.as_ref(), &n) })
                    .await;
                if !failed.is_empty() {
                    eprintln!("restart: not turned back on: {}", failed.join("; "));
                }
                continue;
            }
            let offered = !offer.is_empty();
            let (n, t) = (notice.clone(), turn_on.clone());
            let gone = window.take();
            match cx.update(move |cx| sync(cx, gone, offered, n, t)) {
                Ok(next) => window = next,
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
            Err(_) => {
                notice.dismiss();
                None
            }
        },
        (None, true) => {
            let (w, h) = window_size(false, false);
            let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
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
