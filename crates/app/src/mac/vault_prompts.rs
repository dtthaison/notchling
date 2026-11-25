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
use crate::vault::twins::{Cover, Stage};
use crate::vault::wire::{Approval, Decision, PendingRequest};

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
    let waiting = tickets
        .iter()
        .any(|t| open.approvals.contains_key(t) || open.requests.contains_key(t));
    if !waiting {
        return;
    }
    cx.activate(true);
    for ticket in tickets {
        if let Some(handle) = open.approvals.get(ticket) {
            handle.update(cx, |_, w, _| w.activate_window()).ok();
        }
        if let Some(handle) = open.requests.get(ticket) {
            handle.update(cx, |_, w, _| w.activate_window()).ok();
        }
    }
}

#[derive(Default)]
struct Open {
    approvals: HashMap<String, WindowHandle<VaultApprovalView>>,
    requests: HashMap<String, WindowHandle<VaultRequestView>>,
    shown: HashMap<String, Cover>,
    closed: HashSet<String>,
}

impl Open {
    fn covers(&self, request: &PendingRequest) -> bool {
        self.shown.values().any(|mine| mine.covers(request))
    }

    fn form_for(&self, ticket: &str, pool: &[PendingRequest]) -> String {
        if self.approvals.contains_key(ticket) || self.requests.contains_key(ticket) {
            return ticket.to_owned();
        }
        pool.iter()
            .find(|r| r.id == ticket)
            .and_then(|asked| self.shown.iter().find(|(_, mine)| mine.covers(asked)))
            .map_or_else(|| ticket.to_owned(), |(id, _)| id.clone())
    }

    fn raised(&self, pool: &[PendingRequest], tickets: &[String]) -> Vec<String> {
        tickets.iter().map(|t| self.form_for(t, pool)).collect()
    }

    fn refresh(&mut self, cx: &mut App) {
        let Open {
            requests, shown, ..
        } = self;
        for (id, mine) in shown.iter_mut() {
            let cover = requests
                .get(id)
                .and_then(|handle| handle.update(cx, |view, _, _| view.cover().clone()).ok());
            if let Some(cover) = cover {
                *mine = cover;
            }
        }
    }
}

#[derive(Default)]
struct Tracked {
    announced: HashSet<String>,
    floating: HashSet<String>,
}

impl Tracked {
    fn arrivals(&mut self, waiting: &[String]) -> Vec<String> {
        self.announced.retain(|id| waiting.contains(id));
        self.floating.retain(|id| waiting.contains(id));
        let fresh: Vec<String> = waiting
            .iter()
            .filter(|id| !self.announced.contains(*id))
            .cloned()
            .collect();
        self.announced.extend(fresh.iter().cloned());
        fresh
    }

    fn fall_back(&mut self, ids: &[String]) {
        self.floating.extend(ids.iter().cloned());
    }

    fn for_chat(&self, fresh: &[String], raised: &[String], waiting: &[String]) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        for id in fresh.iter().chain(raised) {
            if waiting.contains(id) && !self.floating.contains(id) && !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids
    }
}

pub fn watch(cx: &mut App, feed: Arc<SwitchFeed>, vault: Arc<Vault>) {
    cx.spawn(async move |cx| {
        let mut open = Open::default();
        let mut tracked = Tracked::default();
        loop {
            cx.background_executor().timer(TICK).await;
            let approvals = pending(&feed.approvals());
            let (f, v) = (feed.clone(), vault.clone());
            let mut now = std::mem::take(&mut open);
            let mut seen = std::mem::take(&mut tracked);
            let raised = take_raised();
            match cx.update(move |cx| {
                now.refresh(cx);
                let pool: Vec<PendingRequest> = f
                    .requests()
                    .iter()
                    .filter_map(PendingRequest::from_event)
                    .collect();
                let requests: Vec<PendingRequest> =
                    pool.iter().filter(|r| !now.covers(r)).cloned().collect();
                let waiting: Vec<String> = approvals
                    .iter()
                    .map(|a| a.id.clone())
                    .chain(requests.iter().map(|r| r.id.clone()))
                    .collect();
                let fresh = seen.arrivals(&waiting);
                let for_chat = seen.for_chat(&fresh, &raised, &waiting);
                if !for_chat.is_empty() && !super::shell::open_chat_vault(cx, &for_chat) {
                    seen.fall_back(&for_chat);
                }
                let approvals = approvals
                    .into_iter()
                    .filter(|a| seen.floating.contains(&a.id))
                    .collect();
                let requests = requests
                    .into_iter()
                    .filter(|r| seen.floating.contains(&r.id))
                    .collect();
                let next = sync(cx, now, (approvals, requests), &pool, (f, v));
                bring_forward(cx, &next, &next.raised(&pool, &raised));
                (next, seen)
            }) {
                Ok((next, seen)) => {
                    open = next;
                    tracked = seen;
                }
                Err(_) => return,
            }
        }
    })
    .detach();
}

fn floating(size_pt: (f32, f32), nth: usize, focus: bool, cx: &App) -> WindowOptions {
    let mut bounds = Bounds::centered(None, size(px(size_pt.0), px(size_pt.1)), cx);
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
        w.setLevel(NSFloatingWindowLevel);
        w.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        w.setHidesOnDeactivate(false);
    }
}

fn send_closed(cx: &mut App, id: String, feed: Arc<SwitchFeed>, vault: Arc<Vault>) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        let switches = feed.switches();
        let sent = id.clone();
        let result = cx
            .background_executor()
            .spawn(async move { vault.answer(&switches, &sent, Decision::Deny) })
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
    (approvals, requests): (Vec<Approval>, Vec<PendingRequest>),
    pool: &[PendingRequest],
    (feed, vault): (Arc<SwitchFeed>, Arc<Vault>),
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
            Ok(true) => {
                handle.update(cx, |_, w, _| w.remove_window()).ok();
            }
            Ok(false) if !waiting.contains(&id) => {
                handle.update(cx, |_, w, _| w.remove_window()).ok();
            }
            Ok(false) => {
                next.approvals.insert(id, handle);
            }
            Err(_) => {
                if waiting.contains(&id) {
                    next.closed.insert(id.clone());
                    send_closed(cx, id, feed.clone(), vault.clone());
                }
            }
        }
    }
    for (id, handle) in was.requests.drain() {
        match handle.update(cx, |view, _, _| view.cover().clone()) {
            Ok(cover) if cover.stays(pool) => {
                next.shown.insert(id.clone(), cover);
                next.requests.insert(id, handle);
            }
            Ok(cover) => {
                handle.update(cx, |_, w, _| w.remove_window()).ok();
                if cover.stage() == Stage::Done && waiting.contains(&id) {
                    next.closed.insert(id);
                }
            }
            Err(_) => {
                if let Some(mut mine) = was.shown.remove(&id) {
                    next.closed.extend(mine.settle(Stage::Done, &feed));
                }
                if waiting.contains(&id) {
                    next.closed.insert(id.clone());
                    feed.dismiss_request(&id);
                }
            }
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
            cx.activate(true);
            next.approvals.insert(id, handle);
        }
    }
    for request in requests {
        let id = request.id.clone();
        if next.requests.contains_key(&id) || next.closed.contains(&id) || next.covers(&request) {
            continue;
        }
        let (f, v) = (feed.clone(), vault.clone());
        let nth = next.requests.len();
        let shown = Cover::new(request.clone());
        let opened = cx
            .open_window(floating(REQUEST_SIZE, nth, false, cx), |window, cx| {
                float_over_everything(window);
                cx.new(|cx| VaultRequestView::live(f, v, request, window, cx))
            })
            .map_err(|e| eprintln!("open the vault request: {e}"))
            .ok();
        if let Some(handle) = opened {
            next.shown.insert(id.clone(), shown);
            next.requests.insert(id, handle);
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn asked(id: &str, name: &str, session: &str) -> PendingRequest {
        use notchling_engine::vault::entry::{Kind, VaultEntry};
        PendingRequest::from_event(&serde_json::json!({"id": id, "session": session,
            "request": {"name": name}, "entry": VaultEntry::new(name, Kind::Token),
            "existing": false}))
        .unwrap()
    }

    fn asked_at(id: &str, name: &str, session: &str, revision: &str) -> PendingRequest {
        use notchling_engine::vault::entry::{Kind, VaultEntry};
        PendingRequest::from_event(&serde_json::json!({"id": id, "session": session,
            "request": {"name": name}, "entry": VaultEntry::new(name, Kind::Token),
            "existing": true, "revision": revision}))
        .unwrap()
    }

    #[test]
    fn a_window_stays_for_the_re_ask_of_its_own_request_and_for_nothing_else() {
        let mut open = Open::default();
        open.shown
            .insert("c1".into(), Cover::new(asked("c1", "gh", "s-1")));
        assert!(open.covers(&asked("c2", "GH", "s-1")));
        assert!(!open.covers(&asked("c2", "gh", "s-2")));
        assert!(!open.covers(&asked("c2", "npm", "s-1")));
        assert!(!open.covers(&asked("c1", "gh", "s-1")));
        assert!(!Open::default().covers(&asked("c2", "gh", "s-1")));
    }

    #[test]
    fn a_ticket_is_announced_once_and_forgotten_when_it_is_answered() {
        let mut tracked = Tracked::default();
        assert_eq!(tracked.arrivals(&ids(&["a", "b"])), ids(&["a", "b"]));
        assert!(tracked.arrivals(&ids(&["a", "b"])).is_empty());
        assert_eq!(tracked.arrivals(&ids(&["b", "c"])), ids(&["c"]));
        assert_eq!(tracked.arrivals(&ids(&["a"])), ids(&["a"]), "back again");
    }

    #[test]
    fn chat_gets_the_new_and_the_raised_tickets_but_never_a_floating_one() {
        let mut tracked = Tracked::default();
        let waiting = ids(&["a", "b", "c"]);
        let fresh = tracked.arrivals(&waiting);
        tracked.floating.insert("b".into());
        assert_eq!(
            tracked.for_chat(&fresh, &ids(&["c", "gone"]), &waiting),
            ids(&["a", "c"])
        );
        assert!(tracked.for_chat(&[], &[], &waiting).is_empty());
    }

    #[test]
    fn when_chat_cannot_open_the_raised_tickets_fall_back_to_the_floating_window() {
        let mut tracked = Tracked::default();
        let waiting = ids(&["a", "c"]);
        let fresh = tracked.arrivals(&waiting);
        tracked.fall_back(&tracked.for_chat(&fresh, &[], &waiting).clone());
        let raised = ids(&["c"]);
        let again = tracked.for_chat(&[], &raised, &waiting);
        assert!(again.is_empty(), "already floating");
        let mut later = Tracked::default();
        later.arrivals(&ids(&["c"]));
        let for_chat = later.for_chat(&[], &raised, &ids(&["c"]));
        assert_eq!(for_chat, raised);
        later.fall_back(&for_chat);
        assert!(later.floating.contains("c"));
    }

    #[test]
    fn a_window_never_hides_a_request_recorded_against_another_state_of_the_secret() {
        let mut open = Open::default();
        open.shown
            .insert("c1".into(), Cover::new(asked_at("c1", "gh", "s-1", "r1")));
        assert!(open.covers(&asked_at("c2", "gh", "s-1", "r1")));
        assert!(!open.covers(&asked_at("c3", "gh", "s-1", "r2")));
        assert!(!open.covers(&asked("c4", "gh", "s-1")));
    }

    #[test]
    fn opening_a_covered_twin_raises_the_window_that_covers_it() {
        let mut open = Open::default();
        open.shown
            .insert("c1".into(), Cover::new(asked("c1", "gh", "s-1")));
        let pool = vec![
            asked("c1", "gh", "s-1"),
            asked("c2", "gh", "s-1"),
            asked("c3", "npm", "s-1"),
        ];
        assert_eq!(
            open.raised(&pool, &ids(&["c2", "c3", "gone"])),
            ids(&["c1", "c3", "gone"])
        );
        assert_eq!(open.form_for("c1", &pool), "c1");
        assert_eq!(Open::default().form_for("c2", &pool), "c2");
    }

    #[test]
    fn a_refused_window_neither_hides_a_twin_nor_is_raised_for_one() {
        let feed = SwitchFeed::fixed(crate::switches::Switches::all_on());
        let mut refused = Cover::new(asked("c1", "gh", "s-1"));
        refused.settle(Stage::Refused, &feed);
        let mut open = Open::default();
        open.shown.insert("c1".into(), refused);
        let pool = vec![asked("c2", "gh", "s-1")];
        assert!(!open.covers(&pool[0]));
        assert_eq!(open.form_for("c2", &pool), "c2");
    }
}
