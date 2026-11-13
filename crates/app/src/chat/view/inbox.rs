use std::sync::Arc;

use gpui::{div, prelude::*, px, Context, Div, Entity};
use notchling_kit::metric::{SIDE_PADDING, SPACE};

use super::sidebar::HEADER_ROW;
use super::ChatView;
use crate::inbox::feed::InboxFeed;
use crate::inbox::renders::{model, NOW_MS};
use crate::inbox::view::InboxPanelView;
use crate::inbox::WIDTH;
use crate::mac::shell::AppInbox;

pub fn app_inbox(cx: &Context<ChatView>) -> Arc<InboxFeed> {
    cx.try_global::<AppInbox>()
        .map(|i| i.0.clone())
        .unwrap_or_else(|| Arc::new(InboxFeed::none()))
}

impl ChatView {
    pub(super) fn toggle_inbox(&mut self, cx: &mut Context<Self>) {
        if self.inbox_panel.take().is_none() {
            let feed = self.inbox.clone();
            self.inbox_panel = Some(cx.new(|cx| InboxPanelView::live(feed, cx)));
        }
        cx.notify();
    }

    pub fn show_inbox(&mut self, cx: &mut Context<Self>) {
        if self.inbox_panel.is_none() {
            self.toggle_inbox(cx);
        }
    }

    fn close_inbox(&mut self, cx: &mut Context<Self>) {
        self.inbox_panel = None;
        cx.notify();
    }

    pub(super) fn inbox_tick(&mut self, cx: &mut Context<Self>) {
        let generation = self.inbox.generation();
        if generation != self.seen_inbox {
            self.seen_inbox = generation;
            self.redraw_chrome(cx);
        }
    }

    pub(super) fn preview_inbox(&mut self, cx: &mut Context<Self>) {
        let feed = Arc::new(InboxFeed::fixed(model()));
        self.inbox = feed.clone();
        self.inbox_panel = Some(cx.new(|_| InboxPanelView::fixed(feed, NOW_MS, false)));
    }
}

pub fn render(panel: Option<&Entity<InboxPanelView>>, cx: &mut Context<ChatView>) -> Option<Div> {
    let panel = panel?;
    Some(
        div()
            .absolute()
            .size_full()
            .child(
                div()
                    .id("inbox-click-away")
                    .absolute()
                    .size_full()
                    .occlude()
                    .on_click(cx.listener(|this, _, _, cx| this.close_inbox(cx))),
            )
            .child(
                div()
                    .absolute()
                    .right(px(SIDE_PADDING))
                    .top(px(HEADER_ROW + SPACE[0]))
                    .bottom(px(SPACE[2]))
                    .w(px(WIDTH))
                    .flex()
                    .flex_col()
                    .justify_start()
                    .child(
                        div()
                            .occlude()
                            .min_h(px(0.0))
                            .flex()
                            .flex_col()
                            .child(panel.clone()),
                    ),
            ),
    )
}
