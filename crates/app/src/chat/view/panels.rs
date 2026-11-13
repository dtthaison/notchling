use std::cell::Cell;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, AnyElement, ClickEvent, Context, CursorStyle, DragMoveEvent, EmptyView,
    Window,
};
use notchling_kit::ui::panel_edge;
use notchling_kit::Appearance;

use super::ChatView;
use crate::chat::panels::{self, Saved, Side, Widths};

const SAVE_AFTER: Duration = Duration::from_millis(400);

pub const SIDEBAR_WIDTH_ENV: &str = "NOTCHLING_RENDER_SIDEBAR_WIDTH";
pub const DETAILS_WIDTH_ENV: &str = "NOTCHLING_RENDER_DETAILS_WIDTH";

#[derive(Clone, Copy)]
pub(super) struct SideEdge(Side);

pub(super) struct State {
    pub preferred: Widths,
    pub drawn: Cell<Widths>,
    pub window: Cell<f32>,
    pub dragging: Cell<Option<Side>>,
    changes: u64,
    unsaved: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            preferred: Widths::default(),
            drawn: Cell::new(Widths::default()),
            window: Cell::new(0.0),
            dragging: Cell::new(None),
            changes: 0,
            unsaved: false,
        }
    }
}

impl ChatView {
    pub(super) fn restore_panels(&mut self, window: &Window) {
        if self.root.fixture.is_some() {
            return;
        }
        let Some(saved) = panels::load(&self.root.data_dir.join("settings.json")) else {
            return;
        };
        let width = f32::from(window.viewport_size().width);
        self.panels.preferred = saved.widths(width);
        self.panels.window.set(width);
        self.sidebar_hidden = saved.sidebar_hidden;
        self.details_hidden = saved.details_hidden;
    }

    pub(super) fn force_panel_widths(&mut self) {
        let from_env = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<f32>().ok());
        if let Some(w) = from_env(SIDEBAR_WIDTH_ENV) {
            self.panels.preferred.sidebar = Side::Sidebar.clamp(w);
        }
        if let Some(w) = from_env(DETAILS_WIDTH_ENV) {
            self.panels.preferred.details = Side::Details.clamp(w);
        }
    }

    pub(super) fn lay_out_panels(&self, window: &Window) -> Widths {
        let width = f32::from(window.viewport_size().width);
        let shown = panels::shown(
            self.panels.preferred,
            !self.sidebar_hidden,
            self.details_visible(),
            width,
        );
        self.panels.drawn.set(shown);
        self.panels.window.set(width);
        shown
    }

    pub(super) fn sidebar_width(&self) -> f32 {
        self.panels.drawn.get().sidebar
    }

    pub(super) fn details_width(&self) -> f32 {
        self.panels.drawn.get().details
    }

    fn drag_panel(&mut self, event: &DragMoveEvent<SideEdge>, cx: &mut Context<Self>) {
        let side = event.drag(cx).0;
        let drawn = self.panels.drawn.get();
        let other = match side {
            Side::Sidebar => drawn.details,
            Side::Details => drawn.sidebar,
        };
        let width = panels::dragged(
            side,
            f32::from(event.event.position.x),
            self.panels.window.get(),
            other,
        );
        self.panels.dragging.set(Some(side));
        if (self.panels.preferred.get(side) - width).abs() > 0.01 {
            self.panels.preferred.set(side, width);
            self.panels_changed(cx);
            cx.notify();
        }
    }

    fn reset_panel(&mut self, side: Side, cx: &mut Context<Self>) {
        self.panels.preferred.set(side, side.default_width());
        self.panels_changed(cx);
        cx.notify();
    }

    pub(super) fn panels_changed(&mut self, cx: &mut Context<Self>) {
        if self.root.fixture.is_some() {
            return;
        }
        self.panels.changes += 1;
        self.panels.unsaved = true;
        let n = self.panels.changes;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_AFTER).await;
            let job = this
                .update(cx, |view, _| {
                    (view.panels.changes == n).then(|| {
                        view.panels.unsaved = false;
                        view.panels_job()
                    })
                })
                .ok()
                .flatten();
            if let Some((path, saved)) = job {
                cx.background_executor()
                    .spawn(async move {
                        if let Err(e) = panels::save(&path, &saved) {
                            eprintln!("save Chat's panels: {e}");
                        }
                    })
                    .await;
            }
        })
        .detach();
    }

    fn panels_job(&self) -> (PathBuf, Saved) {
        (
            self.root.data_dir.join("settings.json"),
            Saved::of(
                self.panels.preferred,
                self.sidebar_hidden,
                self.details_hidden,
                self.panels.window.get(),
            ),
        )
    }

    pub(super) fn save_panels_now(&mut self) {
        if self.root.fixture.is_some() || !self.panels.unsaved {
            return;
        }
        self.panels.unsaved = false;
        let (path, saved) = self.panels_job();
        if let Err(e) = panels::save(&path, &saved) {
            eprintln!("save Chat's panels: {e}");
        }
    }

    pub(super) fn panel_edges(
        &self,
        shown: Widths,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let width = f32::from(window.viewport_size().width);
        let active = cx.has_active_drag();
        if !active {
            self.panels.dragging.set(None);
        }
        let dragging = self.panels.dragging.get().filter(|_| active);
        let sides = [
            (Side::Sidebar, "sidebar-edge", shown.sidebar, shown.sidebar),
            (
                Side::Details,
                "details-edge",
                shown.details,
                width - shown.details,
            ),
        ];
        let mut edges = Vec::new();
        for (side, id, shown_width, x) in sides {
            if shown_width <= 0.0 {
                continue;
            }
            edges.push(
                panel_edge(id, x, dragging == Some(side), a)
                    .on_drag(SideEdge(side), |_, _, _, cx| cx.new(|_| EmptyView))
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        if event.click_count() >= 2 {
                            this.reset_panel(side, cx);
                        }
                    }))
                    .into_any_element(),
            );
        }
        if dragging.is_some() {
            edges.push(
                div()
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .cursor(CursorStyle::ResizeLeftRight)
                    .into_any_element(),
            );
        }
        edges
    }

    pub(super) fn follow_panel_drags<E: InteractiveElement>(
        element: E,
        cx: &mut Context<Self>,
    ) -> E {
        element
            .on_drag_move::<SideEdge>(cx.listener(|this, event, _, cx| this.drag_panel(event, cx)))
    }
}
