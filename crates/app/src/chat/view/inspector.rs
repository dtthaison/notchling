use notchling_locale as t;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use gpui::{
    div, prelude::*, px, relative, AnyElement, App, Context, Div, Global, SharedString, Stateful,
    WeakEntity, Window, WindowId,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SPINNER_MINI as MINI;
use notchling_kit::metric::{METER_BAR, SIDE_PADDING};
use notchling_kit::ui::{
    hsla, icon, label, scrollable, spinner as kit_spinner, KitPart, KitStyled,
};
use notchling_kit::{Appearance, Icon, Ink, Status, TypeRole};

use super::paged_rows::{group_more_row, more_row, subagent_row, More};
use super::select::{self, Scope};
use super::{repo_section, worktrees_ask, ChatView};
use crate::chat::format::short_path;
use crate::chat::inspector::{
    fact_icon, fact_text, inspector_with_fan, Inspector, Mark, ShellRow, SubagentRow, TaskRow,
};
use crate::chat::paging::Counts;
use crate::chat::remote_control::Action;
use crate::chat::shell_list::Listing;
use crate::chat::shell_rows;
use crate::chat::worktrees::GitSection;
use notchling_engine::transcript::model::{BackgroundShell, Conversation};

pub(super) const WIDTH: f32 = crate::chat::panels::DETAILS_DEFAULT;
pub(super) const MARK_BOX: f32 = 14.0;
pub(super) const ROW_LINE: f32 = 16.0;

#[derive(Default)]
struct Folds {
    folded: HashMap<WindowId, HashSet<&'static str>>,
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

#[derive(Clone, Debug, PartialEq)]
pub struct DetailsModel {
    pub i: Inspector,
    pub folder: Option<(String, String)>,
    pub reveal: bool,
    pub git: Option<Arc<GitSection>>,
}

#[cfg(test)]
fn model_of(
    conversation: &Conversation,
    subagents: &[SubagentRow],
    subagent_counts: Counts,
    shells: Option<&Listing>,
    writers: &dyn Fn(&str) -> Vec<i32>,
    folder: Option<(String, String)>,
    git: Option<Arc<GitSection>>,
) -> DetailsModel {
    model_with_fan(
        conversation,
        &[],
        subagents,
        subagent_counts,
        shells,
        writers,
        folder,
        git,
    )
}

#[allow(clippy::too_many_arguments)]
fn model_with_fan(
    conversation: &Conversation,
    fan: &[BackgroundShell],
    subagents: &[SubagentRow],
    subagent_counts: Counts,
    shells: Option<&Listing>,
    writers: &dyn Fn(&str) -> Vec<i32>,
    folder: Option<(String, String)>,
    git: Option<Arc<GitSection>>,
) -> DetailsModel {
    DetailsModel {
        i: inspector_with_fan(
            conversation,
            fan,
            subagents,
            subagent_counts,
            shells,
            writers,
        ),
        folder,
        reveal: true,
        git,
    }
}

pub(super) fn model(view: &ChatView, _: &App) -> DetailsModel {
    let folder = view.selected_session().map(|s| {
        let full = s.agent.cwd.clone();
        (short_path(&full, &view.root.home), full)
    });
    let mut model = model_with_fan(
        &view.conversation,
        &view.fan_shells,
        &view.subagent_rows,
        view.subagent_counts,
        Some(&view.shells),
        &|p: &str| view.shell_probe.writers(p),
        folder,
        worktrees_ask::section_of(view),
    );
    model.reveal = !view.selected_refused(Action::Files);
    for row in model.i.shells.iter_mut() {
        if !row.running && view.shell_probe_stopped(&row.output_path) {
            let (status, word, mark) = shell_rows::ended_look("stopped");
            row.status = status;
            row.status_word = word;
            row.mark = mark;
        }
    }
    model
}

pub fn render(m: &DetailsModel, a: Appearance, cx: &mut Context<ChatView>) -> AnyElement {
    Panel {
        i: m.i.clone(),
        folder: m.folder.clone(),
        reveal: m.reveal,
        a,
        chat: cx.entity().downgrade(),
        git: m.git.clone(),
    }
    .into_any_element()
}

#[derive(IntoElement)]
struct Panel {
    i: Inspector,
    folder: Option<(String, String)>,
    reveal: bool,
    a: Appearance,
    chat: WeakEntity<ChatView>,
    git: Option<Arc<GitSection>>,
}

impl RenderOnce for Panel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let _region = super::render_map::scope(notchling_kit::render_map::Region::Details, cx);
        let a = self.a;
        let id = window.window_handle().window_id();
        let i = &self.i;
        let tasks = if i.tasks.is_empty() {
            empty(t::chat::details_no_tasks(), a)
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
                rows.push(folder_row(shown, full, self.reveal, a));
            }
            rows.extend(i.facts.iter().enumerate().map(|(n, (name, value))| {
                fact_row(
                    format!("fact-{n}"),
                    fact_icon(name),
                    &fact_text(name, value),
                    a,
                )
            }));
            if rows.is_empty() {
                empty(t::chat::details_nothing_read(), a)
            } else {
                div().flex().flex_col().gap(px(6.0)).children(rows)
            }
        };

        let subagents = if i.subagents.is_empty() {
            empty(t::chat::details_none_started(), a)
        } else {
            let mut list = div().flex().flex_col().gap(px(6.0));
            for s in &i.subagents {
                list = list.child(subagent_row(s, a, &self.chat, cx));
                for (group, next) in &s.more_after {
                    list = list.child(group_more_row(group, *next, s.depth, &self.chat, a));
                }
            }
            let next = i.subagent_counts.next_page();
            if next > 0 {
                list = list.child(more_row(
                    More::Subagents,
                    &t::chat::details_load_more(&next.to_string()),
                    &self.chat,
                    a,
                ));
            }
            list
        };

        let shells = if i.shells.is_empty() {
            empty(t::chat::details_none_running(), a)
        } else {
            let mut list = div().flex().flex_col().gap(px(6.0)).children(
                i.shells
                    .iter()
                    .enumerate()
                    .map(|(n, s)| shell_row(n, s, a, &self.chat)),
            );
            let next = i.shell_counts.next_page();
            if next > 0 {
                list = list.child(more_row(
                    More::Shells,
                    &t::chat::details_load_more(&next.to_string()),
                    &self.chat,
                    a,
                ));
            }
            list
        };

        let mut sections = vec![
            ("Tasks", "Task Panel", i.tasks_title(), tasks),
            (
                "Facts",
                "Session Facts",
                t::chat::details_session().to_owned(),
                facts,
            ),
            ("Subagents", "Subagents", i.subagents_title(), subagents),
            ("Shells", "Background Shells", i.shells_title(), shells),
        ];
        if let Some(git) = &self.git {
            sections.push((
                "Git",
                "Git Changes",
                git.title.clone(),
                repo_section::content(git, &self.chat, a),
            ));
        }
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
            .size_full()
            .fill(Ink::Sidebar, a)
            .border_l(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .flex()
            .flex_col()
            .child(select::listener_in(Scope::Details, self.chat.clone()))
            .child(scrollable(column))
            .part_inside("Details Panel", Corner::TopTrailing, a, cx)
    }
}

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

pub(super) fn empty(text: &str, a: Appearance) -> Div {
    label(text, TypeRole::Meta, Ink::TextMuted, a)
}

fn spinner(key: &str, a: Appearance) -> impl IntoElement {
    kit_spinner(
        SharedString::from(format!("inspector-spin-{key}")),
        MINI,
        Ink::Working,
        a,
    )
}

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

pub(super) fn fact_row(key: String, shape: Icon, text: &str, a: Appearance) -> Div {
    fact_with(
        shape,
        select::detail_keyed(key, text, TypeRole::Meta, Ink::Text, a),
        a,
    )
}

fn fact_with(shape: Icon, body: Div, a: Appearance) -> Div {
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
                .child(body.line_height(px(ROW_LINE))),
        )
}

fn folder_row(shown: &str, full: &str, reveal: bool, a: Appearance) -> Div {
    let full = full.to_owned();
    if !reveal {
        return div().child(fact_row("fact-folder".to_owned(), Icon::Folder, shown, a));
    }
    let row = fact_with(Icon::Folder, label(shown, TypeRole::Meta, Ink::Text, a), a);
    div().child(
        row.id("inspector-folder")
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
            div().flex_1().min_w(px(0.0)).child(
                select::detail_keyed(
                    format!("task-{index}-subject"),
                    &task.subject,
                    TypeRole::Meta,
                    ink,
                    a,
                )
                .line_height(px(ROW_LINE)),
            ),
        )
}

fn shell_row(index: usize, s: &ShellRow, a: Appearance, chat: &WeakEntity<ChatView>) -> Div {
    let now_s = super::now_ms() / 1000;
    let stuck = shell_rows::stuck_of(s, now_s);
    let status_word = if s.running {
        shell_rows::running_word(s.started, now_s)
    } else {
        s.status_word.clone()
    };
    let mut text = div().flex_1().min_w(px(0.0)).flex().flex_col().gap(px(2.0));
    if s.title != s.command {
        text = text.child(
            select::detail_keyed(
                format!("shell-{index}-title"),
                &s.title,
                TypeRole::Meta,
                Ink::Text,
                a,
            )
            .line_height(px(ROW_LINE))
            .w_full()
            .elide(),
        );
    }
    div()
        .flex()
        .items_start()
        .gap(px(7.0))
        .child(mark_box(s.mark, &format!("shell-{index}"), a))
        .child(
            text.child(
                select::detail_keyed(
                    format!("shell-{index}-command"),
                    &s.command,
                    TypeRole::Mono,
                    Ink::TextMuted,
                    a,
                )
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
                        select::detail_keyed(
                            format!("shell-{index}-status"),
                            &status_word,
                            TypeRole::Meta,
                            Ink::TextFaint,
                            a,
                        )
                        .line_height(px(ROW_LINE)),
                    )
                    .children(
                        (!s.output_path.is_empty()).then(|| output_link(index, s, a, chat.clone())),
                    )
                    .children(
                        (s.running && !s.output_path.is_empty())
                            .then(|| stop_link(index, s, a, chat.clone())),
                    ),
            )
            .children(stuck.map(|why| {
                select::detail_keyed(
                    format!("shell-{index}-stuck"),
                    &shell_rows::stuck_words(why),
                    TypeRole::Meta,
                    Ink::Needs,
                    a,
                )
                .line_height(px(ROW_LINE))
            })),
        )
}

fn stop_link(
    index: usize,
    s: &ShellRow,
    a: Appearance,
    chat: WeakEntity<ChatView>,
) -> Stateful<Div> {
    let path = s.output_path.clone();
    div()
        .id(SharedString::from(format!("shell-stop-{index}")))
        .cursor_pointer()
        .child(
            label(t::chat::shell_stop(), TypeRole::Meta, Ink::AccentText, a)
                .line_height(px(ROW_LINE)),
        )
        .on_click(move |_, _, cx| {
            chat.update(cx, |this, cx| this.stop_shell(&path, cx)).ok();
        })
}

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
        .child(
            label(t::chat::output(), TypeRole::Meta, Ink::AccentText, a).line_height(px(ROW_LINE)),
        )
        .on_click(move |_, _, cx| {
            chat.update(cx, |this, cx| this.open_shell_output(&name, &path, cx))
                .ok();
        })
}

#[cfg(test)]
mod tests {
    use notchling_engine::transcript::model::{BackgroundShell, TaskEntry, TaskStatus};

    use super::*;
    use crate::chat::inspector::no_writers;

    fn with_task(subject: &str) -> Conversation {
        Conversation {
            tasks: vec![TaskEntry {
                id: "1".into(),
                subject: subject.into(),
                status: TaskStatus::InProgress,
            }],
            ..Conversation::default()
        }
    }

    fn folder() -> Option<(String, String)> {
        Some(("~/w".into(), "/Users/me/w".into()))
    }

    #[test]
    fn the_details_model_is_equal_for_the_same_inputs() {
        let c = with_task("Port");
        let a = model_of(
            &c,
            &[],
            Counts::default(),
            None,
            &no_writers,
            folder(),
            None,
        );
        let b = model_of(
            &with_task("Port"),
            &[],
            Counts::default(),
            None,
            &no_writers,
            folder(),
            None,
        );
        assert_eq!(a, b);
        assert_ne!(
            a,
            model_of(&c, &[], Counts::default(), None, &no_writers, None, None),
            "the folder line"
        );
    }

    #[test]
    fn a_new_task_changes_the_details_model() {
        let before = model_of(
            &with_task("Port"),
            &[],
            Counts::default(),
            None,
            &no_writers,
            folder(),
            None,
        );
        let mut more = with_task("Port");
        more.tasks.push(TaskEntry {
            id: "2".into(),
            subject: "Test".into(),
            status: TaskStatus::Pending,
        });
        assert_ne!(
            before,
            model_of(
                &more,
                &[],
                Counts::default(),
                None,
                &no_writers,
                folder(),
                None
            )
        );
        assert_ne!(
            before,
            model_of(
                &with_task("Port it"),
                &[],
                Counts::default(),
                None,
                &no_writers,
                folder(),
                None
            )
        );
    }

    #[test]
    fn a_shell_writer_appearing_changes_the_details_model() {
        let c = Conversation {
            shells: vec![BackgroundShell {
                id: "b1".into(),
                command: "cargo test".into(),
                title: String::new(),
                output_path: "/tmp/b1.output".into(),
                started: None,
            }],
            ..Conversation::default()
        };
        let idle = model_of(&c, &[], Counts::default(), None, &no_writers, None, None);
        let writing = model_of(
            &c,
            &[],
            Counts::default(),
            None,
            &|p: &str| {
                if p == "/tmp/b1.output" {
                    vec![4242]
                } else {
                    vec![]
                }
            },
            None,
            None,
        );
        assert_ne!(
            idle, writing,
            "a process holds the output file open: Running"
        );
        assert!(writing.i.shells[0].running && !idle.i.shells[0].running);
    }
}
