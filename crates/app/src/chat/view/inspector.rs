//! The inspector: tasks, facts, subagents and background shells of the selected session, the Swift
//! Details panel's rows (a status icon first, small text) with a chevron on each section that folds
//! it. Which sections are folded is kept per window (`Folds`), so it outlives the panel being hidden
//! and the session changing; the Swift panel has no folding, so every section starts open.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use gpui::{
    div, prelude::*, px, relative, App, Context, Div, Global, SharedString, Stateful, WeakEntity,
    Window, WindowId,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SPINNER_MINI as MINI;
use notchling_kit::metric::{METER_BAR, RADIUS_SM, SIDE_PADDING};
use notchling_kit::ui::{
    hsla, icon, label, scrollable, spinner as kit_spinner, KitPart, KitStyled,
};
use notchling_kit::{Appearance, Icon, Ink, Status, TypeRole};

use super::ChatView;
use crate::chat::format::short_path;
use crate::chat::inspector::{
    fact_icon, fact_text, inspector, Fold, Inspector, Mark, ShellRow, SubagentRow, TaskRow,
};

pub(super) const WIDTH: f32 = 260.0;
/// A row's mark sits in a box this wide, so the text lines up whatever the mark.
const MARK_BOX: f32 = 14.0;
/// One line of a row's text. The icon box is this tall too, so the icon sits centred on the first
/// line; left to GPUI's default (about 1.6 times the 11 pt type) the text sat lower than the icon.
const ROW_LINE: f32 = 16.0;

/// Which sections each window has folded, and whether its subagent list shows every finished one.
#[derive(Default)]
struct Folds {
    folded: HashMap<WindowId, HashSet<&'static str>>,
    all_subagents: HashSet<WindowId>,
}

impl Global for Folds {}

fn is_folded(cx: &App, window: WindowId, section: &'static str) -> bool {
    cx.try_global::<Folds>()
        .is_some_and(|f| f.folded.get(&window).is_some_and(|s| s.contains(section)))
}

fn toggle_folded(cx: &mut App, window: WindowId, section: &'static str) {
    let folds = cx.default_global::<Folds>();
    let set = folds.folded.entry(window).or_default();
    if !set.remove(section) {
        set.insert(section);
    }
}

/// The inspector panel, for the selected session.
pub fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> impl IntoElement {
    let i = inspector(&view.conversation, &view.subagents, &|p: &str| {
        view.shell_probe.writers(p)
    });
    let folder = view.selected_session().map(|s| {
        let full = s.agent.cwd.clone();
        (short_path(&full, &view.root.home), full)
    });
    Panel {
        i,
        folder,
        a,
        chat: cx.entity().downgrade(),
        agent_ids: view.subagents.iter().map(|s| s.id.clone()).collect(),
    }
}

#[derive(IntoElement)]
struct Panel {
    i: Inspector,
    /// The session's folder as shown (`~/…`), and as a path.
    folder: Option<(String, String)>,
    a: Appearance,
    /// For a row's actions (Output, opening a subagent).
    chat: WeakEntity<ChatView>,
    /// The agent id of each subagent row, oldest first (what a click opens).
    agent_ids: Vec<String>,
}

impl RenderOnce for Panel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let a = self.a;
        let id = window.window_handle().window_id();
        let i = &self.i;

        let tasks = if i.tasks.is_empty() {
            empty("No tasks", a)
        } else {
            let done = i.tasks.iter().filter(|t| t.status == Status::Done).count();
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(progress(done as f32 / i.tasks.len() as f32, a))
                .children(i.tasks.iter().enumerate().map(|(n, t)| task_row(n, t, a)))
        };

        let facts = {
            let mut rows: Vec<Div> = Vec::new();
            if let Some((shown, full)) = &self.folder {
                rows.push(folder_row(shown, full, a));
            }
            rows.extend(
                i.facts
                    .iter()
                    .map(|(name, value)| fact_row(fact_icon(name), &fact_text(name, value), a)),
            );
            if rows.is_empty() {
                empty("Nothing read yet", a)
            } else {
                div().flex().flex_col().gap(px(6.0)).children(rows)
            }
        };

        let all = cx
            .try_global::<Folds>()
            .is_some_and(|f| f.all_subagents.contains(&id));
        let listing = i.subagent_listing(all);
        let subagents = if i.subagents.is_empty() {
            empty("None started", a)
        } else {
            let mut list = div().flex().flex_col().gap(px(6.0));
            for index in &listing.rows {
                list = list.child(subagent_row(
                    *index,
                    &i.subagents[*index],
                    a,
                    &self.chat,
                    &self.agent_ids,
                ));
            }
            match listing.fold {
                Fold::More(n) => {
                    list = list.child(fold_row(
                        id,
                        "subagents-more",
                        &format!("Show {n} more"),
                        true,
                        a,
                    ));
                }
                Fold::Less => {
                    list = list.child(fold_row(id, "subagents-less", "Show less", false, a));
                }
                Fold::None => {}
            }
            list
        };

        let shells = if i.shells.is_empty() {
            empty("None running", a)
        } else {
            div().flex().flex_col().gap(px(6.0)).children(
                i.shells
                    .iter()
                    .enumerate()
                    .map(|(n, s)| shell_row(n, s, a, &self.chat)),
            )
        };

        let sections = [
            ("Tasks", "Task Panel", i.tasks_title(), tasks),
            ("Facts", "Session Facts", "Session".to_owned(), facts),
            ("Subagents", "Subagents", i.subagents_title(), subagents),
            ("Shells", "Background Shells", i.shells_title(), shells),
        ];
        let mut column = div()
            .id("inspector")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .px(px(SIDE_PADDING))
            .pt(px(16.0))
            .pb(px(SIDE_PADDING))
            .flex()
            .flex_col()
            .gap(px(16.0));
        for (key, part, title, content) in sections {
            column = column.child(section((id, &self.chat), key, part, &title, content, a, cx));
        }

        div()
            .w(px(WIDTH))
            .h_full()
            .flex_shrink_0()
            .fill(Ink::Sidebar, a)
            .border_l(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .flex()
            .flex_col()
            // The header runs over the panel too, so the panel starts under it, level with the
            // conversation.
            .child(scrollable(column))
            .part_inside("Details Panel", Corner::TopTrailing, a, cx)
    }
}

/// A section: its header (a chevron and the title, which folds it on a click) over its content, named
/// for the ? button with the Swift panel's name (`part`). The title keeps its count when folded.
fn section(
    (window, chat): (WindowId, &WeakEntity<ChatView>),
    key: &'static str,
    part: &str,
    title: &str,
    content: Div,
    a: Appearance,
    cx: &App,
) -> Div {
    let folded = is_folded(cx, window, key);
    let chat = chat.clone();
    let chevron = if folded {
        Icon::ChevronRight
    } else {
        Icon::ChevronDown
    };
    let header = div()
        .id(SharedString::from(format!("fold-{key}")))
        .flex()
        .items_center()
        .gap(px(4.0))
        .cursor_pointer()
        .child(icon(chevron, 9.0, Ink::TextFaint, a))
        .child(label(title, TypeRole::Section, Ink::TextFaint, a))
        .on_click(move |_, window, cx| {
            toggle_folded(cx, window.window_handle().window_id(), key);
            // The fold is read by the Details Panel alone: draw it, not the whole window.
            chat.update(cx, |v, cx| v.redraw_details(cx)).ok();
        });
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(header)
        .when(!folded, |d| d.child(content))
        .part_at(part, Corner::TopTrailing, a, cx)
}

fn empty(text: &str, a: Appearance) -> Div {
    label(text, TypeRole::Meta, Ink::TextMuted, a)
}

/// The mini spinner on a running row (Swift `controlSize(.mini)`).
fn spinner(key: &str, a: Appearance) -> impl IntoElement {
    kit_spinner(
        SharedString::from(format!("inspector-spin-{key}")),
        MINI,
        Ink::Working,
        a,
    )
}

/// The mark box that leads a row.
fn mark_box(mark: Mark, key: &str, a: Appearance) -> Div {
    let holder = div()
        .w(px(MARK_BOX))
        .h(px(ROW_LINE))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center();
    match mark {
        Mark::Spinner => holder.child(spinner(key, a)),
        Mark::Icon(shape, ink) => holder.child(icon(shape, 11.0, ink, a)),
    }
}

/// The Swift fact row: a 10 pt faint icon in a 14 pt box, then the text in the meta type.
fn fact_row(shape: Icon, text: &str, a: Appearance) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(8.0))
        .child(
            div()
                .w(px(MARK_BOX))
                .h(px(ROW_LINE))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(shape, 10.0, Ink::TextFaint, a)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .child(label(text, TypeRole::Meta, Ink::Text, a).line_height(px(ROW_LINE))),
        )
}

/// The session's folder: a fact row that reveals it in Finder.
fn folder_row(shown: &str, full: &str, a: Appearance) -> Div {
    let full = full.to_owned();
    div().child(
        fact_row(Icon::Folder, shown, a)
            .id("inspector-folder")
            .cursor_pointer()
            .on_click(move |_, _, cx| cx.reveal_path(Path::new(&full))),
    )
}

fn progress(share: f32, a: Appearance) -> Div {
    div()
        .w_full()
        .h(px(METER_BAR))
        .rounded(px(METER_BAR / 2.0))
        .fill(Ink::Hairline, a)
        .child(
            div()
                .h_full()
                .w(relative(share.clamp(0.0, 1.0)))
                .rounded(px(METER_BAR / 2.0))
                .fill(Ink::Accent, a),
        )
}

fn task_row(index: usize, task: &TaskRow, a: Appearance) -> Div {
    let mark = match task.status {
        Status::Done => Mark::Icon(Icon::Done, Ink::Done),
        Status::Working => Mark::Spinner,
        Status::Needs => Mark::Icon(Icon::Clock, Ink::TextFaint),
    };
    let ink = if task.status == Status::Done {
        Ink::TextFaint
    } else {
        Ink::Text
    };
    div()
        .flex()
        .items_start()
        .gap(px(6.0))
        .child(mark_box(mark, &format!("task-{index}"), a))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .child(label(&task.subject, TypeRole::Meta, ink, a).line_height(px(ROW_LINE))),
        )
}

/// A subagent: a click opens its transcript in place of the conversation. The row has the
/// pointing hand and the kit's hover wash, like a step in the conversation.
fn subagent_row(
    index: usize,
    s: &SubagentRow,
    a: Appearance,
    chat: &WeakEntity<ChatView>,
    ids: &[String],
) -> Stateful<Div> {
    let (chat, agent) = (chat.clone(), ids.get(index).cloned());
    div()
        .id(SharedString::from(format!("subagent-{index}")))
        // The wash reaches past the text a little, so it doesn't hug the letters.
        .mx(px(-4.0))
        .px(px(4.0))
        .py(px(2.0))
        .rounded(px(RADIUS_SM))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(Ink::Raised.color(a).with_alpha(0.6))))
        .on_click(move |_, _, cx| {
            if let Some(agent) = &agent {
                chat.update(cx, |this, cx| this.open_subagent(agent, cx))
                    .ok();
            }
        })
        .flex()
        .items_start()
        .gap(px(7.0))
        .child(mark_box(s.mark, &format!("agent-{index}"), a))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    label(&s.description, TypeRole::Meta, Ink::Text, a).line_height(px(ROW_LINE)),
                )
                .child(
                    label(&s.detail, TypeRole::Meta, Ink::TextFaint, a).line_height(px(ROW_LINE)),
                ),
        )
}

fn shell_row(index: usize, s: &ShellRow, a: Appearance, chat: &WeakEntity<ChatView>) -> Div {
    let mut text = div().flex_1().min_w(px(0.0)).flex().flex_col().gap(px(2.0));
    if s.title != s.command {
        text = text.child(
            label(&s.title, TypeRole::Meta, Ink::Text, a)
                .line_height(px(ROW_LINE))
                .w_full()
                .elide(),
        );
    }
    // A command can run to pages (a whole prompt passed to a tool): one line of title and two of
    // command, cut with an ellipsis; Output shows the rest.
    div()
        .flex()
        .items_start()
        .gap(px(7.0))
        .child(mark_box(s.mark, &format!("shell-{index}"), a))
        .child(
            text.child(
                label(&s.command, TypeRole::Mono, Ink::TextMuted, a)
                    .line_height(px(ROW_LINE))
                    .w_full()
                    .elide()
                    .line_clamp(2),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        label(&s.status_word, TypeRole::Meta, Ink::TextFaint, a)
                            .line_height(px(ROW_LINE)),
                    )
                    .child(output_link(index, s, a, chat.clone())),
            ),
        )
}

/// Output: the command's output in a terminal tab, live while it runs.
fn output_link(
    index: usize,
    s: &ShellRow,
    a: Appearance,
    chat: WeakEntity<ChatView>,
) -> Stateful<Div> {
    let (name, path) = (s.title.clone(), s.output_path.clone());
    div()
        .id(SharedString::from(format!("shell-output-{index}")))
        .cursor_pointer()
        .child(label("Output", TypeRole::Meta, Ink::AccentText, a).line_height(px(ROW_LINE)))
        .on_click(move |_, _, cx| {
            chat.update(cx, |this, cx| this.open_shell_output(&name, &path, cx))
                .ok();
        })
}

/// "Show N more" / "Show less" under the subagents: a small accent row.
fn fold_row(window: WindowId, key: &'static str, text: &str, expand: bool, a: Appearance) -> Div {
    div().child(
        div()
            .id(key)
            .cursor_pointer()
            .child(label(text, TypeRole::Meta, Ink::AccentText, a))
            .on_click(move |_, window_ref, cx| {
                let folds = cx.default_global::<Folds>();
                if expand {
                    folds.all_subagents.insert(window);
                } else {
                    folds.all_subagents.remove(&window);
                }
                window_ref.refresh();
            }),
    )
}
