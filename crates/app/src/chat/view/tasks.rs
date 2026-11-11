//! The Tasks bar: the session's task list pinned over the top of the conversation column, the
//! Swift `TaskPanel`. Folded it is one line (a checklist icon, "Tasks 1/3", the task under way,
//! a thin progress bar, a chevron); a click opens the list under it, each task with its status
//! icon, and a second click folds it. A session with no tasks has no bar. × hides it until the
//! list changes, and a finished list goes on its own a little after its last task is done
//! (`chat::tasks_bar`).

use std::time::Duration;

use gpui::{div, prelude::*, px, relative, Context, Div, FontWeight};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{card, icon, icon_button, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, Status, TypeRole};

use super::ChatView;
use crate::chat::inspector::{task_rows, TaskRow, TaskSummary};
use crate::chat::tasks_bar::{self, DONE_FOR_MS};
use crate::mac::gate;
use crate::phone::bridge::now_ms;

/// The id the bar's open state is kept under in `ChatView::expanded`.
pub const OPEN_ID: &str = "tasks-open";

/// The bar sits this far under the header.
const TOP: f32 = 16.0;
/// The folded bar's height: 10 + 10 padding, a 16 pt line, the 1 pt border each side.
const FOLDED_HEIGHT: f32 = 38.0;
/// How much room the conversation leaves above its first row so the folded bar doesn't cover it.
pub const INSET: f32 = TOP + FOLDED_HEIGHT;
/// The progress bar's width and height (Swift `ProgressBar().frame(width: 90)`, 5 pt tall).
const PROGRESS_WIDTH: f32 = 90.0;
const PROGRESS_HEIGHT: f32 = 5.0;
/// An open list scrolls past this height rather than covering the conversation.
const LIST_MAX_HEIGHT: f32 = 260.0;

/// Whether the conversation has a bar, so it leaves room for it.
pub fn shown(view: &ChatView) -> bool {
    !view.conversation.tasks.is_empty()
        // The session's tasks aren't the subagent's.
        && view.open_subagent.is_none()
        && view
            .selected
            .as_ref()
            .and_then(|id| view.tasks_seen.get(id))
            .is_some_and(|seen| tasks_bar::visible(seen, now_ms()))
}

impl ChatView {
    /// Before a draw: what Chat saw of the selected session's task list, and a redraw for when a
    /// finished list is due to go.
    pub(super) fn sync_tasks(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        if self.conversation.tasks.is_empty() || self.loading {
            return;
        }
        let signature: Vec<String> = self
            .conversation
            .tasks
            .iter()
            .map(|t| format!("{}:{:?}", t.id, t.status))
            .collect();
        let rows = task_rows(&self.conversation);
        let all_done = TaskSummary::of(&rows).is_some_and(|s| s.all_done());
        let now = now_ms();
        let seen = tasks_bar::observe(
            self.tasks_seen.get(&id),
            &signature.join(","),
            all_done,
            self.selected_working(),
            now,
        );
        let newly_done = seen.done_at == Some(now);
        self.tasks_seen.insert(id, seen);
        if newly_done {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(DONE_FOR_MS as u64 + 50))
                    .await;
                this.update(cx, |_, cx| cx.notify()).ok();
            })
            .detach();
        }
    }

    /// ×: hides the bar until the list changes.
    fn dismiss_tasks(&mut self, cx: &mut Context<Self>) {
        if let Some(seen) = self
            .selected
            .as_ref()
            .and_then(|id| self.tasks_seen.get_mut(id))
        {
            seen.dismissed = true;
        }
        cx.notify();
    }
}

/// The bar, absolute at the top of the conversation column; `None` without tasks.
pub fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    if !shown(view) {
        return None;
    }
    let tasks = task_rows(&view.conversation);
    let summary = TaskSummary::of(&tasks)?;
    let open = view.tasks_open();

    let close = gate::tip_text(
        icon_button(
            "tasks-bar-close",
            Icon::Close,
            11.0,
            ButtonKind::Ghost,
            PartState::Rest,
            a,
        )
        .w(px(20.0))
        .h(px(20.0)),
        "Hide the tasks until the list changes".into(),
        a,
    )
    .on_click(cx.listener(|this, _, _, cx| {
        cx.stop_propagation();
        this.dismiss_tasks(cx);
    }));
    let heading = div()
        .id("tasks-bar-toggle")
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_pointer()
        .on_click(cx.listener(|this, _, _, cx| this.toggle(OPEN_ID, cx)))
        .child(icon(Icon::Task, 12.0, Ink::AccentText, a))
        .child(label(&summary.title(), TypeRole::Heading, Ink::Text, a).flex_shrink_0())
        .child(match &summary.current {
            Some(subject) => label(
                &format!("Now: {subject}"),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            )
            .flex_1()
            .min_w(px(0.0))
            .elide(),
            None if summary.all_done() => label("All done", TypeRole::Meta, Ink::Done, a).flex_1(),
            None => div().flex_1(),
        })
        .child(progress(summary.fraction(), a))
        .child(icon(
            if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            9.0,
            Ink::TextFaint,
            a,
        ));

    let list = open.then(|| {
        div()
            .id("tasks-list")
            .w_full()
            .max_h(px(LIST_MAX_HEIGHT))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .children(tasks.iter().map(|t| task_line(t, a)))
    });

    let bar = card(a)
        .w_full()
        .px(px(14.0))
        .py(px(10.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(heading)
                .child(close),
        )
        .children(list)
        .part_at("Tasks Bar", Corner::Bottom, a, cx);
    Some(
        div()
            .absolute()
            .top(px(TOP))
            .left(px(SIDE_PADDING))
            .right(px(SIDE_PADDING))
            .child(bar),
    )
}

/// The thin bar: a faint track, the accent fill at least a dot wide.
fn progress(share: f32, a: Appearance) -> Div {
    div()
        .w(px(PROGRESS_WIDTH))
        .h(px(PROGRESS_HEIGHT))
        .flex_shrink_0()
        .rounded(px(PROGRESS_HEIGHT / 2.0))
        .fill(Ink::Raised, a)
        .child(
            div()
                .h_full()
                .w(relative(share.clamp(0.0, 1.0)))
                .min_w(px(4.0))
                .rounded(px(PROGRESS_HEIGHT / 2.0))
                .fill(Ink::Accent, a),
        )
}

/// A task in the open list (Swift `TaskStatusIcon` and a body-type subject): done is a green
/// check and faint struck-through text, under way a half-filled accent circle and semibold text,
/// waiting an empty faint circle.
fn task_line(task: &TaskRow, a: Appearance) -> Div {
    let (shape, ink) = match task.status {
        Status::Done => (Icon::Done, Ink::Done),
        Status::Working => (Icon::Halfway, Ink::Accent),
        Status::Needs => (Icon::Circle, Ink::TextFaint),
    };
    let text_ink = if task.status == Status::Done {
        Ink::TextFaint
    } else {
        Ink::Text
    };
    let mut text = label(&task.subject, TypeRole::Body, text_ink, a)
        .flex_1()
        .min_w(px(0.0));
    match task.status {
        Status::Done => text = text.line_through(),
        Status::Working => text = text.font_weight(FontWeight::SEMIBOLD),
        Status::Needs => {}
    }
    div()
        .w_full()
        .flex()
        .items_start()
        .gap(px(8.0))
        // The icon box is the body line's height, so a wrapped subject keeps it on the first line.
        .child(
            div()
                .h(px(17.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .child(icon(shape, 12.0, ink, a)),
        )
        .child(text)
}
