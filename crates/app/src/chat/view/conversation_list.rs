use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, list, prelude::*, px, AnyElement, Context, ListOffset, ListSizingBehavior, ListState,
    SharedString, Window,
};
use notchling_engine::transcript::model::Conversation;
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::render_map::Region;
use notchling_kit::ui::{scrollable_list, window_appearance};

use super::conversation::{live_group_of, render_row, COLUMN_PADDING_VERTICAL, ROW_GAP};
use super::conversation_view::{ConversationInput, ConversationView};
use super::row_select::autoscroll_speed;
use crate::chat::follow::Scroll;
use crate::chat::log::{Change, Splice};
use crate::chat::rows::{row_of, Row};

pub const OVERDRAW: f32 = 600.0;

pub(super) type Pending = (Arc<Conversation>, Arc<Conversation>, Change);

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Synced {
    pub keys: bool,
    pub replaced: bool,
}

struct Built {
    shown: Arc<Conversation>,
    session: Option<String>,
    agent: Option<String>,
    keys: Vec<SharedString>,
    tail: Vec<(bool, String)>,
    live: Option<SharedString>,
    inset: bool,
}

pub(super) struct ListRows {
    pub state: ListState,
    built: Option<Built>,
    #[cfg(test)]
    pub spliced: std::cell::RefCell<Vec<(Range<usize>, usize)>>,
}

pub(super) fn keys_of(shown: &Conversation, outgoing: &[String]) -> Vec<SharedString> {
    let mut keys: Vec<SharedString> = shown
        .items
        .iter()
        .map(|item| SharedString::from(item.id.clone()))
        .collect();
    let tail = shown.queued.len() + outgoing.len();
    keys.extend((0..tail).map(|k| {
        let kind = if k < shown.queued.len() {
            "queued"
        } else {
            "sending"
        };
        SharedString::from(format!("{kind}-{k}"))
    }));
    keys
}

fn tail_of(shown: &Conversation, outgoing: &[String]) -> Vec<(bool, String)> {
    let queued = shown.queued.iter().map(|text| (false, text.clone()));
    let sending = outgoing.iter().map(|text| (true, text.clone()));
    queued.chain(sending).collect()
}

pub(super) fn row_at(input: &ConversationInput, i: usize) -> Option<Row> {
    let shown = &*input.shown;
    if let Some(item) = shown.items.get(i) {
        return Some(row_of(item));
    }
    let k = i - shown.items.len();
    let user = |text: &String, sending: bool| Row::User {
        text: text.clone(),
        images: 0,
        queued: true,
        sending,
    };
    match shown.queued.get(k) {
        Some(text) => Some(user(text, false)),
        None => input
            .outgoing
            .get(k - shown.queued.len())
            .map(|text| user(text, true)),
    }
}

pub(super) fn at_bottom(state: &ListState) -> bool {
    if state.logical_scroll_top().item_ix >= state.item_count() {
        return true;
    }
    if state.viewport_bounds().size.height <= px(0.0) {
        return false;
    }
    Scroll {
        offset_y: f32::from(state.scroll_px_offset_for_scrollbar().y),
        max_y: f32::from(state.max_offset_for_scrollbar().height),
    }
    .at_bottom()
}

pub(super) fn scroll_to_end(state: &ListState) {
    state.scroll_to(ListOffset {
        item_ix: state.item_count(),
        offset_in_item: px(0.0),
    });
}

pub(super) fn goes_to_end(at_bottom: bool, forced: bool, changed: bool) -> bool {
    forced || (at_bottom && changed)
}

pub(super) fn scroll_target(offset: f32, max: f32, delta: f32) -> f32 {
    (offset.min(max) + delta).clamp(0.0, max.max(0.0))
}

const DRAG_TICK: Duration = Duration::from_millis(16);

enum Plan {
    Rebuild,
    Reset,
    Splices(Vec<Splice>),
    Nothing,
}

impl ListRows {
    pub fn new(state: ListState) -> ListRows {
        ListRows {
            state,
            built: None,
            #[cfg(test)]
            spliced: std::cell::RefCell::default(),
        }
    }

    pub fn at_bottom(&self) -> bool {
        at_bottom(&self.state)
    }

    pub fn keys(&self) -> &[SharedString] {
        self.built.as_ref().map_or(&[], |b| &b.keys)
    }

    pub fn places(&self) -> HashMap<SharedString, usize> {
        self.keys()
            .iter()
            .enumerate()
            .map(|(i, key)| (key.clone(), i))
            .collect()
    }

    #[cfg(test)]
    pub fn spliced(&self) -> Vec<(Range<usize>, usize)> {
        self.spliced.borrow().clone()
    }

    fn splice(&self, range: Range<usize>, count: usize) {
        let top = self.state.logical_scroll_top();
        let kept = top.item_ix < self.state.item_count()
            && range.contains(&top.item_ix)
            && range.len() == count;
        #[cfg(test)]
        self.spliced.borrow_mut().push((range.clone(), count));
        self.state.splice(range, count);
        if kept {
            self.state.scroll_to(top);
        }
    }

    pub fn remeasure(&self, items: Range<usize>) {
        let count = items.len();
        self.splice(items, count);
    }

    pub fn sync(
        &mut self,
        input: &ConversationInput,
        change: Option<&Pending>,
        force_follow: bool,
    ) -> Synced {
        let keys = keys_of(&input.shown, &input.outgoing);
        let tail = tail_of(&input.shown, &input.outgoing);
        let live = live_group_of(input).and_then(|i| keys.get(i).cloned());
        let session = input.session.as_ref().map(|s| s.id.clone());
        let agent = input.subagent.as_ref().map(|s| s.agent_id.clone());
        let rows = keys.len();
        let next = Built {
            shown: input.shown.clone(),
            session,
            agent,
            keys,
            tail,
            live,
            inset: input.tasks_inset,
        };
        let Some(old) = self.built.take() else {
            self.state.reset(rows + 2);
            self.built = Some(next);
            return Synced {
                keys: true,
                replaced: true,
            };
        };
        let plan = if old.session != next.session || old.agent != next.agent {
            Plan::Rebuild
        } else if Arc::ptr_eq(&old.shown, &next.shown) {
            Plan::Nothing
        } else {
            match change {
                Some((from, to, change))
                    if Arc::ptr_eq(from, &old.shown) && Arc::ptr_eq(to, &next.shown) =>
                {
                    match change {
                        Change::Same => Plan::Nothing,
                        Change::Items { splices, .. } if fits(splices, &old, &next) => {
                            Plan::Splices(splices.clone())
                        }
                        _ => Plan::Reset,
                    }
                }
                _ => Plan::Reset,
            }
        };
        let replaced = !Arc::ptr_eq(&old.shown, &next.shown);
        let changed = !matches!(plan, Plan::Nothing)
            || old.tail != next.tail
            || old.live != next.live
            || old.inset != next.inset;
        if !matches!(plan, Plan::Nothing) {
            super::pane::LIST_CHANGES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let follow = goes_to_end(self.at_bottom(), force_follow, changed);
        let mut keys_changed = old.keys != next.keys;
        match plan {
            Plan::Rebuild => {
                self.state.reset(rows + 2);
                keys_changed = true;
            }
            Plan::Reset => {
                let anchor = (!follow).then(|| self.anchor(&old)).flatten();
                self.state.reset(rows + 2);
                if let Some((key, offset)) = anchor {
                    let at = match key {
                        None => Some(0),
                        Some(key) => next.keys.iter().position(|k| *k == key).map(|i| i + 1),
                    };
                    if let Some(item_ix) = at {
                        self.state.scroll_to(ListOffset {
                            item_ix,
                            offset_in_item: offset,
                        });
                    }
                }
                keys_changed = true;
            }
            Plan::Splices(splices) => {
                for splice in splices.iter().rev() {
                    self.splice(splice.old.start + 1..splice.old.end + 1, splice.new.len());
                }
                self.tail(&old, &next, input);
                self.measured_again(&old, &next);
            }
            Plan::Nothing => {
                self.tail(&old, &next, input);
                self.measured_again(&old, &next);
            }
        }
        if follow {
            scroll_to_end(&self.state);
        }
        self.built = Some(next);
        Synced {
            keys: keys_changed,
            replaced,
        }
    }

    fn anchor(&self, old: &Built) -> Option<(Option<SharedString>, gpui::Pixels)> {
        let top = self.state.logical_scroll_top();
        if top.item_ix >= self.state.item_count() {
            return None;
        }
        match top.item_ix {
            0 => Some((None, top.offset_in_item)),
            n => old
                .keys
                .get(n - 1)
                .map(|key| (Some(key.clone()), top.offset_in_item)),
        }
    }

    fn tail(&self, old: &Built, next: &Built, input: &ConversationInput) {
        if old.tail != next.tail {
            let start = input.shown.items.len() + 1;
            self.splice(start..start + old.tail.len(), next.tail.len());
        }
    }

    fn measured_again(&self, old: &Built, next: &Built) {
        if old.live != next.live {
            for key in [&old.live, &next.live].into_iter().flatten() {
                if let Some(i) = next.keys.iter().position(|k| k == key) {
                    self.remeasure(i + 1..i + 2);
                }
            }
        }
        if old.inset != next.inset {
            self.remeasure(0..1);
        }
    }
}

fn fits(splices: &[Splice], old: &Built, next: &Built) -> bool {
    let (before, after) = (old.shown.items.len(), next.shown.items.len());
    let mut at = 0;
    let mut len = before;
    for s in splices {
        if s.old.start < at || s.old.end > before {
            return false;
        }
        at = s.old.end;
        len = len - s.old.len() + s.new.len();
    }
    len == after
}

impl ConversationView {
    fn drag_speed(&self) -> f32 {
        let frame = self.s.sel.borrow();
        let (true, Some(pointer)) = (frame.selection.dragging, frame.pointer) else {
            return 0.0;
        };
        let bounds = self.s.list.viewport_bounds();
        autoscroll_speed(
            f32::from(pointer.y),
            f32::from(bounds.top()),
            f32::from(bounds.bottom()),
        )
    }

    pub(super) fn drag_scroll(&mut self, cx: &mut Context<Self>) {
        if self.s.sel.borrow().scrolling || self.drag_speed() == 0.0 {
            return;
        }
        self.s.sel.borrow_mut().scrolling = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(DRAG_TICK).await;
            if !matches!(this.update(cx, |view, cx| view.drag_tick(cx)), Ok(true)) {
                break;
            }
        })
        .detach();
    }

    fn drag_tick(&mut self, cx: &mut Context<Self>) -> bool {
        let speed = self.drag_speed();
        if speed == 0.0 {
            self.s.sel.borrow_mut().scrolling = false;
            return false;
        }
        let (offset, max) = (
            -f32::from(self.s.list.scroll_px_offset_for_scrollbar().y),
            f32::from(self.s.list.max_offset_for_scrollbar().height),
        );
        self.s.list.set_offset_from_scrollbar(gpui::point(
            px(0.0),
            px(-scroll_target(offset, max, speed)),
        ));
        let mut frame = self.s.sel.borrow_mut();
        if let Some(spot) = frame.pointer.and_then(|p| frame.spot_at(p)) {
            frame.drag_to(spot);
        }
        drop(frame);
        cx.notify();
        true
    }
    pub(super) fn render_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(input) = self.input.as_ref() else {
            return div().into_any_element();
        };
        let rows = input.shown.items.len() + input.shown.queued.len() + input.outgoing.len();
        if ix == 0 {
            let inset = if input.tasks_inset {
                super::tasks::INSET
            } else {
                0.0
            };
            return div()
                .h(px(inset + COLUMN_PADDING_VERTICAL - ROW_GAP))
                .into_any_element();
        }
        if ix > rows {
            return div().h(px(COLUMN_PADDING_VERTICAL)).into_any_element();
        }
        let i = ix - 1;
        let Some(row) = row_at(input, i) else {
            return div().into_any_element();
        };
        let _region = super::render_map::scope(Region::Conversation, cx);
        let a = window_appearance(window);
        let live = live_group_of(input) == Some(i);
        let built_at = super::scroll_probe::enabled().then(std::time::Instant::now);
        let element = render_row(input, &self.s, i, &row, live, a, cx);
        if let Some(at) = built_at {
            super::scroll_probe::built(at.elapsed());
        }
        let mut item = div()
            .w_full()
            .px(px(SIDE_PADDING))
            .pt(px(ROW_GAP))
            .child(element);
        if super::scroll_probe::enabled() {
            let key = super::row_select::key_of(&input.shown.items, i, &row);
            item = item.relative().child(super::scroll_probe::row_canvas(key));
        }
        item.into_any_element()
    }

    pub(super) fn list_column(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = list(self.s.list.clone(), cx.processor(Self::render_item))
            .with_sizing_behavior(ListSizingBehavior::Auto)
            .flex_1()
            .min_h(px(0.0));
        scrollable_list("conversation", rows, &self.s.list)
    }
}

#[cfg(test)]
#[path = "conversation_list_tests.rs"]
mod tests;
