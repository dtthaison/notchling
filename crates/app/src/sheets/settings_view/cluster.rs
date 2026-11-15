use std::sync::Arc;
use std::time::Duration;

use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_kit::metric::RADIUS_SM;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, label, KitStyled};
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

    fn choose_failover(&mut self, value: &str, cx: &mut Context<Self>) {
        let Some(feed) = self.cluster_feed(cx) else {
            return;
        };
        let Some(request) = self.failover_row(cx).and_then(|r| r.change(value)) else {
            return;
        };
        self.error = None;
        cx.spawn(async move |this, cx| {
            let sent = cx
                .background_executor()
                .spawn(async move {
                    let sent = feed.send(&request);
                    feed.refresh();
                    sent
                })
                .await;
            this.update(cx, |view, cx| {
                if let Err(e) = sent {
                    view.error = Some(e);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn cluster_section(
        &self,
        n: &mut usize,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let shown = self.failover_row(cx)?;
        *n += 1;
        let i = *n;
        let choices = div()
            .flex()
            .p(px(2.0))
            .gap(px(2.0))
            .rounded(px(RADIUS_SM))
            .fill(Ink::Raised, a)
            .children(shown.options.iter().map(|(value, title)| {
                let value = (*value).to_owned();
                let kind = if shown.selected == value {
                    ButtonKind::Secondary
                } else {
                    ButtonKind::Ghost
                };
                button(
                    SharedString::from(format!("setting-{i}-{value}")),
                    title,
                    kind,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.choose_failover(&value, cx)))
            }));
        let notes = shown
            .lines
            .iter()
            .map(|line| label(line, TypeRole::Meta, Ink::TextFaint, a))
            .chain(
                shown
                    .not_ready
                    .map(|text| label(text, TypeRole::Meta, Ink::Needs, a)),
            );
        let body = card(a).flex().flex_col().child(
            div()
                .px(px(14.0))
                .py(px(8.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .min_h(px(24.0))
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .child(label(shown.title, TypeRole::Body, Ink::Text, a).flex_1())
                        .child(choices),
                )
                .children(notes),
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
