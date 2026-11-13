//! The windows for the vault's prompts (part U9): while an agent waits for approval to use a
//! secret, or asks for one to be added, a floating card over everything, whether or not Chat is
//! open, as the Swift `VaultPrompt` panels were (one panel per request). A short loop looks at the
//! switch feed a few times a second:
//!
//! - each pending approval gets a window of its own, bound to it; the app is brought forward once,
//!   when that window opens, never again on a tick. The window goes when the daemon took the
//!   answer. Closing it is Deny, sent to the daemon.
//! - each pending request gets a form window of its own, opened without taking the focus or
//!   bringing the app forward (an agent asked; the person comes to it). Closing it is Cancel
//!   (`vault.request.cancel`).
//!
//! Only the app follows `vault`, so only the app can show these; an agent-tier client never sees
//! them.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    point, prelude::*, px, size, App, AsyncApp, Bounds, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind, WindowOptions,
};
use objc2_app_kit::{NSFloatingWindowLevel, NSWindowCollectionBehavior};

use super::capture::ns_window;
use crate::switches::feed::SwitchFeed;
use crate::vault::approval::{pending, PROMPT_SIZE};
use crate::vault::approval_view::VaultApprovalView;
use crate::vault::prompt_view::{VaultRequestView, REQUEST_SIZE};
use crate::vault::store::Vault;
use crate::vault::wire::{Approval, Decision, PendingRequest};

/// How often the feed is looked at.
const TICK: Duration = Duration::from_millis(200);

static RAISE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub fn raise(ticket: &str) {
    RAISE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(ticket.to_owned());
}

fn take_raised() -> Vec<String> {
    std::mem::take(&mut *RAISE.lock().unwrap_or_else(|e| e.into_inner()))
}

fn bring_forward(cx: &mut App, open: &Open, tickets: &[String]) {
    let mut any = false;
    for ticket in tickets {
        if let Some(handle) = open.approvals.get(ticket) {
            any |= handle.update(cx, |_, w, _| w.activate_window()).is_ok();
        }
        if let Some(handle) = open.requests.get(ticket) {
            any |= handle.update(cx, |_, w, _| w.activate_window()).is_ok();
        }
    }
    if any {
        cx.activate(true);
    }
}

/// What the loop keeps between ticks.
#[derive(Default)]
struct Open {
    approvals: HashMap<String, WindowHandle<VaultApprovalView>>,
    requests: HashMap<String, WindowHandle<VaultRequestView>>,
    /// Closed by the person: Deny or Cancel is on its way; never opened again.
    closed: HashSet<String>,
}

/// Starts the loop that opens and closes the prompt windows for `feed`'s pending items.
pub fn watch(cx: &mut App, feed: Arc<SwitchFeed>, vault: Arc<Vault>) {
    cx.spawn(async move |cx| {
        let mut open = Open::default();
        loop {
            cx.background_executor().timer(TICK).await;
            let approvals = pending(&feed.approvals());
            let requests: Vec<PendingRequest> = feed
                .requests()
                .iter()
                .filter_map(PendingRequest::from_event)
                .collect();
            let (f, v) = (feed.clone(), vault.clone());
            let now = std::mem::take(&mut open);
            let raised = take_raised();
            match cx.update(move |cx| {
                let next = sync(cx, now, approvals, requests, f, v);
                bring_forward(cx, &next, &raised);
                next
            }) {
                Ok(next) => open = next,
                // The app is quitting.
                Err(_) => return,
            }
        }
    })
    .detach();
}

fn floating(size_pt: (f32, f32), nth: usize, focus: bool, cx: &App) -> WindowOptions {
    let mut bounds = Bounds::centered(None, size(px(size_pt.0), px(size_pt.1)), cx);
    // Each further prompt a little lower and to the right, so none hides another.
    let step = px(24.0 * (nth % 8) as f32);
    bounds.origin = point(bounds.origin.x + step, bounds.origin.y + step);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        window_background: WindowBackgroundAppearance::Opaque,
        focus,
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

/// Sends what closing a window means (Deny for an approval, Cancel for a request) off the UI
/// thread; the item leaves the feed's list only once the daemon took it.
fn send_closed(cx: &mut App, id: String, approval: bool, feed: Arc<SwitchFeed>, vault: Arc<Vault>) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        let switches = feed.switches();
        let sent = id.clone();
        let result = cx
            .background_executor()
            .spawn(async move {
                if approval {
                    vault.answer(&switches, &sent, Decision::Deny)
                } else {
                    vault.cancel_request(&switches, &sent)
                }
            })
            .await;
        match result {
            Ok(()) => feed.resolve(&id),
            Err(e) => eprintln!("vault: closing {id} wasn't sent ({e})"),
        }
    })
    .detach();
}

fn sync(
    cx: &mut App,
    mut was: Open,
    approvals: Vec<Approval>,
    requests: Vec<PendingRequest>,
    feed: Arc<SwitchFeed>,
    vault: Arc<Vault>,
) -> Open {
    let waiting: HashSet<String> = approvals
        .iter()
        .map(|a| a.id.clone())
        .chain(requests.iter().map(|r| r.id.clone()))
        .collect();
    let mut next = Open {
        closed: was.closed.intersection(&waiting).cloned().collect(),
        ..Open::default()
    };
    for (id, handle) in was.approvals.drain() {
        match handle.update(cx, |view, _, _| view.resolved()) {
            // Answered and taken, or no longer waiting: the window goes.
            Ok(true) => {
                handle.update(cx, |_, w, _| w.remove_window()).ok();
            }
            Ok(false) if !waiting.contains(&id) => {
                handle.update(cx, |_, w, _| w.remove_window()).ok();
            }
            Ok(false) => {
                next.approvals.insert(id, handle);
            }
            // Closed by the person: that is Deny.
            Err(_) => {
                if waiting.contains(&id) {
                    next.closed.insert(id.clone());
                    send_closed(cx, id, true, feed.clone(), vault.clone());
                }
            }
        }
    }
    for (id, handle) in was.requests.drain() {
        let alive = handle.update(cx, |_, _, _| ()).is_ok();
        if alive && waiting.contains(&id) {
            next.requests.insert(id, handle);
        } else if alive {
            handle.update(cx, |_, w, _| w.remove_window()).ok();
        } else if waiting.contains(&id) {
            // Closed by the person: that is Cancel.
            next.closed.insert(id.clone());
            send_closed(cx, id, false, feed.clone(), vault.clone());
        }
    }
    for approval in approvals {
        let id = approval.id.clone();
        if next.approvals.contains_key(&id) || next.closed.contains(&id) {
            continue;
        }
        let (f, v) = (feed.clone(), vault.clone());
        let nth = next.approvals.len();
        let opened = cx
            .open_window(floating(PROMPT_SIZE, nth, true, cx), |window, cx| {
                float_over_everything(window);
                cx.new(|cx| VaultApprovalView::live(&approval, f, v, window, cx))
            })
            .map_err(|e| eprintln!("open the vault approval: {e}"))
            .ok();
        if let Some(handle) = opened {
            // Once, as this window opens: a prompt nobody sees is an agent that waits five minutes
            // to be refused. Allow stays unarmed for a moment after.
            cx.activate(true);
            next.approvals.insert(id, handle);
        }
    }
    for request in requests {
        let id = request.id.clone();
        if next.requests.contains_key(&id) || next.closed.contains(&id) {
            continue;
        }
        let (f, v) = (feed.clone(), vault.clone());
        let nth = next.requests.len();
        let opened = cx
            .open_window(floating(REQUEST_SIZE, nth, false, cx), |window, cx| {
                float_over_everything(window);
                cx.new(|cx| VaultRequestView::live(f, v, request, cx))
            })
            .map_err(|e| eprintln!("open the vault request: {e}"))
            .ok();
        if let Some(handle) = opened {
            next.requests.insert(id, handle);
        }
    }
    next
}
