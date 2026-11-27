use std::sync::Arc;
use std::time::Duration;

use gpui::{div, prelude::*, px, Context, Div};
use notchling_kit::ui::{card, label};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::SettingsView;
use crate::cluster::feed::ClusterFeed;
use crate::mac::shell::AppCluster;
use crate::sheets::settings_cluster::{row, FailoverRow};

const POLL: Duration = Duration::from_millis(500);

impl SettingsView {
    pub fn with_cluster(mut self, feed: Arc<ClusterFeed>) -> SettingsView {
        self.cluster = Some(feed);
        self
    }

    fn cluster_feed(&self, cx: &Context<Self>) -> Option<Arc<ClusterFeed>> {
        self.cluster
            .clone()
            .or_else(|| cx.try_global::<AppCluster>().map(|g| g.0.clone()))
    }

    pub fn watch_cluster(&mut self, cx: &mut Context<Self>) {
        let Some(feed) = self.cluster_feed(cx) else {
            return;
        };
        let mut seen = feed.generation();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            let generation = feed.generation();
            let moved = generation != seen;
            seen = generation;
            let alive = this.update(cx, |_, cx| {
                if moved {
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    pub(super) fn failover_row(&self, cx: &Context<Self>) -> Option<FailoverRow> {
        row(&self.cluster_feed(cx)?.snapshot())
    }

    pub(super) fn cluster_section(&self, a: Appearance, cx: &Context<Self>) -> Option<Div> {
        let shown = self.failover_row(cx)?;
        let body = card(a).flex().flex_col().child(
            div()
                .px(px(14.0))
                .py(px(8.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(div().min_h(px(24.0)).flex().items_center().child(label(
                    shown.title,
                    TypeRole::Body,
                    Ink::Text,
                    a,
                )))
                .child(label(shown.line, TypeRole::Meta, Ink::TextFaint, a)),
        );
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(label(
                    t::settings::section_cluster(),
                    TypeRole::Section,
                    Ink::TextFaint,
                    a,
                ))
                .child(body),
        )
    }
}
