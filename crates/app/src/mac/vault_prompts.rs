//! The windows for the vault's prompts (part U9): while an agent waits for approval to use a
//! secret, or asks for one to be added, a floating card over everything, whether or not Chat is
//! open, as the Swift `VaultPrompt` panels were. A short loop looks at the switch feed a few times
//! a second: a pending approval opens the approval window and none closes it; each pending
//! request gets a form window of its own until the daemon resolves it or the person closes it.
//! Only the app follows `vault`, so only the app can show these; an agent-tier client never sees
//! them.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    prelude::*, px, size, App, Bounds, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions,
};
use objc2_app_kit::{NSFloatingWindowLevel, NSWindowCollectionBehavior};

use super::capture::ns_window;
use crate::switches::feed::SwitchFeed;
use crate::vault::approval::PROMPT_SIZE;
use crate::vault::prompt_view::{VaultApprovalView, VaultRequestView, REQUEST_SIZE};
use crate::vault::store::Vault;
use crate::vault::wire::PendingRequest;

/// How often the feed is looked at.
const TICK: Duration = Duration::from_millis(200);

/// Starts the loop that opens and closes the prompt windows for `feed`'s pending items.
pub fn watch(cx: &mut App, feed: Arc<SwitchFeed>, vault: Arc<Vault>) {
    cx.spawn(async move |cx| {
        let mut approval: Option<WindowHandle<VaultApprovalView>> = None;
        let mut requests: HashMap<String, WindowHandle<VaultRequestView>> = HashMap::new();
        loop {
            cx.background_executor().timer(TICK).await;
            let waiting = !feed.approvals().is_empty();
            let pending: Vec<PendingRequest> = feed
                .requests()
                .iter()
                .filter_map(PendingRequest::from_event)
                .collect();
            let (feed_now, vault_now) = (feed.clone(), vault.clone());
            let gone = approval.take();
            let open_requests = std::mem::take(&mut requests);
            let next = cx.update(move |cx| {
                let approval =
                    sync_approval(cx, gone, waiting, feed_now.clone(), vault_now.clone());
                let requests = sync_requests(cx, open_requests, pending, feed_now, vault_now);
                (approval, requests)
            });
            match next {
                Ok((a, r)) => {
                    approval = a;
                    requests = r;
                }
                // The app is quitting.
                Err(_) => return,
            }
        }
    })
    .detach();
}

fn floating(size_pt: (f32, f32), cx: &App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(size_pt.0), px(size_pt.1)), cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        window_background: WindowBackgroundAppearance::Opaque,
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    }
}

fn float_over_everything(window: &mut gpui::Window) {
    if let Some(w) = ns_window(window) {
        // Over other windows and on the Space the person is on, like the panels were.
        w.setLevel(NSFloatingWindowLevel);
        w.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        w.setHidesOnDeactivate(false);
    }
}

/// Opens the approval window when an approval waits and there is none, closes it when none is.
fn sync_approval(
    cx: &mut App,
    window: Option<WindowHandle<VaultApprovalView>>,
    waiting: bool,
    feed: Arc<SwitchFeed>,
    vault: Arc<Vault>,
) -> Option<WindowHandle<VaultApprovalView>> {
    match (window, waiting) {
        (Some(handle), false) => {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
            None
        }
        // A window the person closed any other way: the next tick opens it again, since the
        // daemon still waits (closing counts as nothing; Deny is a click).
        (Some(handle), true) => handle.update(cx, |_, _, _| ()).ok().map(|()| handle),
        (None, true) => {
            let opened = cx
                .open_window(floating(PROMPT_SIZE, cx), |window, cx| {
                    float_over_everything(window);
                    cx.new(|cx| VaultApprovalView::live(feed, vault, cx))
                })
                .map_err(|e| eprintln!("open the vault approval: {e}"))
                .ok();
            if opened.is_some() {
                // A prompt nobody sees is an agent that waits five minutes to be refused.
                cx.activate(true);
            }
            opened
        }
        (None, false) => None,
    }
}

/// One form window per pending request; one the daemon resolved (or whose window the person
/// closed, which cancelled it) goes away.
fn sync_requests(
    cx: &mut App,
    mut open: HashMap<String, WindowHandle<VaultRequestView>>,
    pending: Vec<PendingRequest>,
    feed: Arc<SwitchFeed>,
    vault: Arc<Vault>,
) -> HashMap<String, WindowHandle<VaultRequestView>> {
    let still: Vec<&str> = pending.iter().map(|p| p.id.as_str()).collect();
    let mut kept = HashMap::new();
    for (id, handle) in open.drain() {
        if still.contains(&id.as_str()) {
            // Still open: keep it. Closed by the person: forget it (the view cancelled it).
            if handle.update(cx, |_, _, _| ()).is_ok() {
                kept.insert(id, handle);
            }
        } else {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
    }
    for request in pending {
        if kept.contains_key(&request.id) {
            continue;
        }
        let (feed, vault) = (feed.clone(), vault.clone());
        let id = request.id.clone();
        let opened = cx
            .open_window(floating(REQUEST_SIZE, cx), |window, cx| {
                float_over_everything(window);
                cx.new(|cx| VaultRequestView::live(feed, vault, request, cx))
            })
            .map_err(|e| eprintln!("open the vault request: {e}"))
            .ok();
        if let Some(handle) = opened {
            cx.activate(true);
            kept.insert(id, handle);
        }
    }
    kept
}
