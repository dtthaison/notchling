//! The inspector beside the conversation: tasks, the session's facts, subagents, background shells.

use std::path::Path;

use notchling_engine::transcript::model::{Conversation, TaskStatus};
use notchling_engine::transcript::shells::{self, ShellState};
use notchling_engine::transcript::subagents::{Status as SubagentStatus, Subagent};
use notchling_kit::{Icon, Ink, Status};

use super::format::{model_name, short_path, tokens};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRow {
    pub subject: String,
    pub status: Status,
    pub icon: Icon,
}

/// The status mark that leads a row: a spinner while it runs, else an icon in an ink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Spinner,
    Icon(Icon, Ink),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubagentRow {
    pub description: String,
    /// The Swift line: "Running · sonnet high · Explore" (the usual general-purpose type is left out).
    pub detail: String,
    pub status: Status,
    pub status_word: String,
    pub running: bool,
    pub mark: Mark,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellRow {
    pub title: String,
    pub command: String,
    pub status: Status,
    pub status_word: String,
    pub running: bool,
    pub mark: Mark,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inspector {
    pub tasks: Vec<TaskRow>,
    /// Label and value, in display order.
    pub facts: Vec<(&'static str, String)>,
    pub subagents: Vec<SubagentRow>,
    pub shells: Vec<ShellRow>,
}

fn task_row(subject: &str, status: TaskStatus) -> TaskRow {
    let (status, icon) = match status {
        TaskStatus::Pending => (Status::Needs, Icon::More),
        TaskStatus::InProgress => (Status::Working, Icon::Working),
        TaskStatus::Completed => (Status::Done, Icon::Check),
    };
    TaskRow {
        subject: subject.to_owned(),
        status,
        icon,
    }
}

fn subagent_row(agent: &Subagent) -> SubagentRow {
    let (status, word, mark) = match agent.status {
        SubagentStatus::Running => (Status::Working, "Running", Mark::Spinner),
        SubagentStatus::Completed => (Status::Done, "Done", Mark::Icon(Icon::Done, Ink::Done)),
        SubagentStatus::Failed => (
            Status::Needs,
            "Failed",
            Mark::Icon(Icon::Failed, Ink::Expired),
        ),
        SubagentStatus::Stopped => (
            Status::Needs,
            "Stopped",
            Mark::Icon(Icon::Failed, Ink::TextFaint),
        ),
        SubagentStatus::Quiet => (
            Status::Done,
            "Not running",
            Mark::Icon(Icon::Clock, Ink::TextFaint),
        ),
    };
    let model = [agent.model.as_deref(), agent.effort.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    let kind = (agent.agent_type != "general-purpose" && !agent.agent_type.is_empty())
        .then_some(agent.agent_type.as_str());
    let detail = [
        Some(word),
        (!model.is_empty()).then_some(model.as_str()),
        kind,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    SubagentRow {
        description: agent.description.clone(),
        detail,
        status,
        status_word: word.to_owned(),
        running: agent.is_running(),
        mark,
    }
}

/// How many subagents show while the list is folded to the latest finished ones.
pub const FINISHED_SHOWN: usize = 3;

/// The row under a subagent list that changes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fold {
    /// Nothing to expand or fold back.
    None,
    /// "Show N more".
    More(usize),
    /// "Show less".
    Less,
}

/// What the Subagents section lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    /// Indices into the subagents, in the order shown (newest first).
    pub rows: Vec<usize>,
    /// How many are left out.
    pub hidden: usize,
    pub fold: Fold,
}

/// Which subagents to list. `running` says, for each one oldest first, whether it still runs.
/// While any run, only those are listed. With none running, the `FINISHED_SHOWN` most recent
/// finished ones, then "Show N more"; `expanded` lists them all, then "Show less".
pub fn list_subagents(running: &[bool], expanded: bool) -> Listing {
    let newest_first = (0..running.len()).rev();
    if running.iter().any(|r| *r) {
        let rows: Vec<usize> = newest_first.filter(|i| running[*i]).collect();
        return Listing {
            hidden: running.len() - rows.len(),
            rows,
            fold: Fold::None,
        };
    }
    let all: Vec<usize> = newest_first.collect();
    if all.len() <= FINISHED_SHOWN {
        return Listing {
            rows: all,
            hidden: 0,
            fold: Fold::None,
        };
    }
    if expanded {
        return Listing {
            rows: all,
            hidden: 0,
            fold: Fold::Less,
        };
    }
    let hidden = all.len() - FINISHED_SHOWN;
    Listing {
        rows: all[..FINISHED_SHOWN].to_vec(),
        hidden,
        fold: Fold::More(hidden),
    }
}

/// A section title with its count the way the Swift panel writes it: "Subagents · 2 running", or
/// "Subagents · 7" when none run, and the bare name when there are none.
pub fn counted_title(name: &str, running: usize, total: usize) -> String {
    if total == 0 {
        name.to_owned()
    } else if running > 0 {
        format!("{name} · {running} running")
    } else {
        format!("{name} · {total}")
    }
}

impl Inspector {
    /// "Tasks · 1/3" (done of all), as Swift writes it.
    pub fn tasks_title(&self) -> String {
        if self.tasks.is_empty() {
            return "Tasks".to_owned();
        }
        let done = self
            .tasks
            .iter()
            .filter(|t| t.status == Status::Done)
            .count();
        format!("Tasks · {done}/{}", self.tasks.len())
    }

    pub fn subagents_title(&self) -> String {
        let running = self.subagents.iter().filter(|s| s.running).count();
        counted_title("Subagents", running, self.subagents.len())
    }

    pub fn shells_title(&self) -> String {
        let running = self.shells.iter().filter(|s| s.running).count();
        counted_title("Shells", running, self.shells.len())
    }

    /// The listing of subagents (oldest first in `subagents`).
    pub fn subagent_listing(&self, expanded: bool) -> Listing {
        let running: Vec<bool> = self.subagents.iter().map(|s| s.running).collect();
        list_subagents(&running, expanded)
    }
}

/// Facts as the inspector lists them: only what the transcript says.
pub fn facts(conversation: &Conversation, home: &Path) -> Vec<(&'static str, String)> {
    let f = &conversation.facts;
    let mut out: Vec<(&'static str, String)> = Vec::new();
    if let Some(branch) = &f.branch {
        out.push(("Branch", branch.clone()));
    }
    if let Some(model) = &f.model {
        out.push(("Model", model_name(model)));
    }
    if let Some(effort) = &f.effort {
        out.push(("Effort", effort.clone()));
    }
    if f.turns > 0 {
        out.push(("Turns", f.turns.to_string()));
    }
    if f.input_tokens + f.output_tokens > 0 {
        out.push((
            "Tokens",
            format!(
                "{} in · {} out",
                tokens(f.input_tokens),
                tokens(f.output_tokens)
            ),
        ));
    }
    if f.commands > 0 {
        out.push(("Commands", f.commands.to_string()));
    }
    if !f.files.is_empty() {
        let names: Vec<String> = f.files.iter().map(|p| short_path(p, home)).collect();
        out.push(("Files", names.join("\n")));
    }
    out
}

/// The inspector for a conversation. Shells are read-only here: running means a process still has
/// the output file open, which only the live app can ask (`writers`); a reader passes none, so a
/// shell with no notification shows as ended.
pub fn inspector(
    conversation: &Conversation,
    subagents: &[Subagent],
    home: &Path,
    writers: &dyn Fn(&str) -> Vec<i32>,
) -> Inspector {
    Inspector {
        tasks: conversation
            .tasks
            .iter()
            .map(|t| task_row(&t.subject, t.status))
            .collect(),
        facts: facts(conversation, home),
        subagents: subagents.iter().map(subagent_row).collect(),
        shells: conversation
            .shells
            .iter()
            .map(|shell| {
                let state = shells::state(shell, &conversation.finished_agents, writers);
                let (status, word, mark) = match state {
                    ShellState::Running => (Status::Working, "Running".to_owned(), Mark::Spinner),
                    ShellState::Ended(how) => {
                        let ok = how == "completed";
                        (
                            if ok { Status::Done } else { Status::Needs },
                            capitalized(&how),
                            if ok {
                                Mark::Icon(Icon::Done, Ink::Done)
                            } else {
                                Mark::Icon(Icon::Failed, Ink::Expired)
                            },
                        )
                    }
                };
                ShellRow {
                    title: if shell.title.is_empty() {
                        shell.command.clone()
                    } else {
                        shell.title.clone()
                    },
                    command: shell.command.clone(),
                    status,
                    status_word: word,
                    running: status == Status::Working,
                    mark,
                }
            })
            .collect(),
    }
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The icon on a fact row, the Swift panel's (folder, branch, cpu, turns, terminal, tokens); the
/// effort and the files, which Swift has no row for, get the gauge and the file.
pub fn fact_icon(label: &str) -> Icon {
    match label {
        "Folder" => Icon::Folder,
        "Branch" => Icon::Branch,
        "Model" => Icon::Cpu,
        "Effort" => Icon::Gauge,
        "Turns" => Icon::Turns,
        "Tokens" => Icon::Tokens,
        "Commands" => Icon::Terminal,
        _ => Icon::File,
    }
}

/// What a fact row says beside its icon: the Swift panel writes "2 turns" and "3 commands", where
/// the facts carry the bare number.
pub fn fact_text(label: &str, value: &str) -> String {
    let plural = |word: &str| {
        if value == "1" {
            format!("1 {word}")
        } else {
            format!("{value} {word}s")
        }
    };
    match label {
        "Turns" => plural("turn"),
        "Commands" => plural("command"),
        "Effort" => format!("{value} effort"),
        _ => value.to_owned(),
    }
}

/// No process can be asked about: nothing writes.
pub fn no_writers(_: &str) -> Vec<i32> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::transcript::model::{BackgroundShell, SessionFacts, TaskEntry};

    #[test]
    fn tasks_facts_and_shells() {
        let mut c = Conversation {
            tasks: vec![
                TaskEntry {
                    id: "1".into(),
                    subject: "Fix".into(),
                    status: TaskStatus::Completed,
                },
                TaskEntry {
                    id: "2".into(),
                    subject: "Port".into(),
                    status: TaskStatus::InProgress,
                },
            ],
            facts: SessionFacts {
                branch: Some("main".into()),
                model: Some("claude-opus-5-5".into()),
                effort: Some("high".into()),
                input_tokens: 15_400,
                output_tokens: 900,
                turns: 2,
                files: vec!["/Users/me/w/a.rs".into()],
                commands: 3,
                ..SessionFacts::default()
            },
            shells: vec![
                BackgroundShell {
                    id: "b1".into(),
                    command: "cargo test".into(),
                    title: "Run tests".into(),
                    output_path: "/tmp/b1.output".into(),
                    started: None,
                },
                BackgroundShell {
                    id: "b2".into(),
                    command: "sleep 9".into(),
                    title: String::new(),
                    output_path: "/tmp/b2.output".into(),
                    started: None,
                },
            ],
            ..Conversation::default()
        };
        c.finished_agents.insert("b1".into(), "completed".into());
        let i = inspector(&c, &[], Path::new("/Users/me"), &no_writers);
        assert_eq!(i.tasks[0].status, Status::Done);
        assert_eq!(i.tasks[1].icon, Icon::Working);
        assert_eq!(
            i.facts,
            [
                ("Branch", "main".to_owned()),
                ("Model", "Opus 5.5".to_owned()),
                ("Effort", "high".to_owned()),
                ("Turns", "2".to_owned()),
                ("Tokens", "15.4k in · 900 out".to_owned()),
                ("Commands", "3".to_owned()),
                ("Files", "~/w/a.rs".to_owned()),
            ]
        );
        assert_eq!(i.shells[0].title, "Run tests");
        assert_eq!(
            (i.shells[0].status, i.shells[0].status_word.as_str()),
            (Status::Done, "Completed")
        );
        assert_eq!(i.shells[1].title, "sleep 9", "no description: the command");
        assert_eq!(i.shells[1].status_word, "Ended");
    }

    #[test]
    fn subagent_rows() {
        let agent = Subagent {
            id: "abg".into(),
            path: String::new(),
            description: "Survey the repo".into(),
            agent_type: "Explore".into(),
            model: Some("sonnet".into()),
            effort: Some("high".into()),
            background: true,
            tool_use_id: None,
            started: 0.0,
            updated: 0.0,
            status: SubagentStatus::Completed,
        };
        let i = inspector(
            &Conversation::default(),
            &[agent],
            Path::new("/x"),
            &no_writers,
        );
        assert_eq!(
            i.subagents[0],
            SubagentRow {
                description: "Survey the repo".into(),
                detail: "Done · sonnet high · Explore".into(),
                status: Status::Done,
                status_word: "Done".into(),
                running: false,
                mark: Mark::Icon(Icon::Done, Ink::Done),
            }
        );
        assert_eq!(i.subagents_title(), "Subagents · 1");
        assert!(i.facts.is_empty() && i.tasks.is_empty() && i.shells.is_empty());
    }

    fn agent(status: SubagentStatus, kind: &str) -> Subagent {
        Subagent {
            id: "a".into(),
            path: String::new(),
            description: "d".into(),
            agent_type: kind.into(),
            model: None,
            effort: None,
            background: false,
            tool_use_id: None,
            started: 0.0,
            updated: 0.0,
            status,
        }
    }

    #[test]
    fn a_running_subagent_has_the_spinner_and_the_plain_type_is_left_out() {
        let i = inspector(
            &Conversation::default(),
            &[
                agent(SubagentStatus::Running, "general-purpose"),
                agent(SubagentStatus::Failed, "Plan"),
            ],
            Path::new("/x"),
            &no_writers,
        );
        assert_eq!(i.subagents[0].mark, Mark::Spinner);
        assert_eq!(i.subagents[0].detail, "Running");
        assert_eq!(i.subagents[1].detail, "Failed · Plan");
        assert_eq!(i.subagents_title(), "Subagents · 1 running");
    }

    #[test]
    fn titles_carry_the_counts() {
        assert_eq!(counted_title("Subagents", 2, 5), "Subagents · 2 running");
        assert_eq!(counted_title("Subagents", 0, 7), "Subagents · 7");
        assert_eq!(counted_title("Shells", 0, 0), "Shells");
        let mut c = Conversation::default();
        assert_eq!(
            inspector(&c, &[], Path::new("/x"), &no_writers).tasks_title(),
            "Tasks"
        );
        c.tasks = vec![
            TaskEntry {
                id: "1".into(),
                subject: "a".into(),
                status: TaskStatus::Completed,
            },
            TaskEntry {
                id: "2".into(),
                subject: "b".into(),
                status: TaskStatus::Pending,
            },
        ];
        assert_eq!(
            inspector(&c, &[], Path::new("/x"), &no_writers).tasks_title(),
            "Tasks · 1/2"
        );
    }

    #[test]
    fn shell_rows_know_if_they_run() {
        let mut c = Conversation::default();
        for (id, ended, path) in [("b1", true, "/done"), ("b2", false, "/live")] {
            c.shells.push(BackgroundShell {
                id: id.into(),
                command: "x".into(),
                title: String::new(),
                output_path: path.into(),
                started: None,
            });
            if ended {
                c.finished_agents.insert(id.into(), "completed".into());
            }
        }
        let writers = |path: &str| if path == "/live" { vec![1] } else { Vec::new() };
        let i = inspector(&c, &[], Path::new("/x"), &writers);
        assert_eq!(i.shells[0].mark, Mark::Icon(Icon::Done, Ink::Done));
        assert!(i.shells[1].running && i.shells[1].mark == Mark::Spinner);
        assert_eq!(i.shells_title(), "Shells · 1 running");
    }

    #[test]
    fn fact_rows_read_like_the_swift_panel() {
        assert_eq!(fact_icon("Folder"), Icon::Folder);
        assert_eq!(fact_icon("Model"), Icon::Cpu);
        assert_eq!(fact_icon("Commands"), Icon::Terminal);
        assert_eq!(fact_icon("Files"), Icon::File);
        assert_eq!(fact_text("Turns", "1"), "1 turn");
        assert_eq!(fact_text("Turns", "2"), "2 turns");
        assert_eq!(fact_text("Commands", "3"), "3 commands");
        assert_eq!(fact_text("Effort", "high"), "high effort");
        assert_eq!(
            fact_text("Tokens", "15.4k in · 900 out"),
            "15.4k in · 900 out"
        );
    }

    // Oldest first, as the subagents come: `true` is a running one.
    #[test]
    fn while_any_run_only_those_are_listed() {
        let l = list_subagents(&[false, true, false, true, false], false);
        assert_eq!(l.rows, [3, 1], "newest first");
        assert_eq!((l.hidden, l.fold), (3, Fold::None));
        // Expanding changes nothing while some run.
        assert_eq!(list_subagents(&[false, true, false, true, false], true), l);
    }

    #[test]
    fn with_none_running_the_three_latest_show_then_show_more() {
        let l = list_subagents(&[false; 7], false);
        assert_eq!(l.rows, [6, 5, 4]);
        assert_eq!((l.hidden, l.fold), (4, Fold::More(4)));
        let l = list_subagents(&[false; 7], true);
        assert_eq!(l.rows, [6, 5, 4, 3, 2, 1, 0]);
        assert_eq!((l.hidden, l.fold), (0, Fold::Less));
    }

    #[test]
    fn three_or_fewer_finished_show_whole_with_no_fold_row() {
        for n in 0..=FINISHED_SHOWN {
            let l = list_subagents(&vec![false; n], false);
            assert_eq!(l.rows.len(), n);
            assert_eq!((l.hidden, l.fold), (0, Fold::None));
            assert_eq!(list_subagents(&vec![false; n], true), l);
        }
        assert_eq!(list_subagents(&[false; 4], false).fold, Fold::More(1));
    }
}
