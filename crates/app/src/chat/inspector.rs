//! The inspector beside the conversation: tasks, the session's facts, subagents, background shells.

use notchling_engine::transcript::codex;
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::model::{Conversation, TaskStatus};
use notchling_engine::transcript::shells::{self, ShellState};
use notchling_engine::transcript::subagents::{
    Origin as SubagentOrigin, Status as SubagentStatus, Subagent,
};
use notchling_kit::ui::ToolMark;
use notchling_kit::{Icon, Ink, Status};

use super::format::{model_name, tokens};

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
    /// The tool that runs it, whose logo leads the row (the status is in `detail`).
    pub tool: ToolMark,
    /// What a running subagent is doing now, from its own transcript ("Running cargo test…").
    pub doing: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellRow {
    pub title: String,
    pub command: String,
    pub status: Status,
    pub status_word: String,
    pub running: bool,
    pub mark: Mark,
    /// The file Claude Code writes the command's output to (Output opens it).
    pub output_path: String,
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

/// The session's tasks as rows, in list order.
pub fn task_rows(conversation: &Conversation) -> Vec<TaskRow> {
    conversation
        .tasks
        .iter()
        .map(|t| task_row(&t.subject, t.status))
        .collect()
}

/// What the pinned Tasks bar says: done of all, the task under way, how far along.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskSummary {
    pub done: usize,
    pub total: usize,
    /// The first task in progress ("Now: …").
    pub current: Option<String>,
}

impl TaskSummary {
    /// `None` for no tasks: the bar isn't drawn.
    pub fn of(tasks: &[TaskRow]) -> Option<TaskSummary> {
        if tasks.is_empty() {
            return None;
        }
        Some(TaskSummary {
            done: tasks.iter().filter(|t| t.status == Status::Done).count(),
            total: tasks.len(),
            current: tasks
                .iter()
                .find(|t| t.status == Status::Working)
                .map(|t| t.subject.clone()),
        })
    }

    /// "Tasks 1/3" (the Swift bar's heading; the Details section writes "Tasks · 1/3").
    pub fn title(&self) -> String {
        format!("Tasks {}/{}", self.done, self.total)
    }

    /// The progress bar's fill, 0 to 1.
    pub fn fraction(&self) -> f32 {
        self.done as f32 / self.total as f32
    }

    pub fn all_done(&self) -> bool {
        self.done == self.total
    }
}

pub fn subagent_row(agent: &Subagent) -> SubagentRow {
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
    // A job handed to Codex or Antigravity names the tool before its model ("Running · Codex ·
    // gpt-5.5 high"); a Claude Code subagent says its model, then its type.
    let handed_over = agent.origin != SubagentOrigin::Claude;
    let parts = if handed_over {
        [
            Some(word),
            Some(agent.agent_type.as_str()).filter(|t| !t.is_empty()),
            (!model.is_empty()).then_some(model.as_str()),
        ]
    } else {
        [
            Some(word),
            (!model.is_empty()).then_some(model.as_str()),
            kind,
        ]
    };
    // Its own transcript, read incrementally and cached: what it does now and how much context
    // it holds. Antigravity's log is read whole when opened; too much for every redraw.
    let read = (!agent.path.is_empty())
        .then(|| match agent.origin {
            SubagentOrigin::Claude => Some(conversation::read_subagent_shared(&agent.path)),
            SubagentOrigin::Codex => Some(codex::read_shared(&agent.path)),
            SubagentOrigin::Antigravity => None,
        })
        .flatten();
    // Context as a bare number ("232.3k").
    let context = read
        .as_ref()
        .map(|c| c.facts.context_tokens)
        .filter(|n| *n > 0)
        .map(tokens);
    let detail = parts
        .into_iter()
        .flatten()
        .chain(context.as_deref())
        .collect::<Vec<_>>()
        .join(" · ");
    SubagentRow {
        description: agent.description.clone(),
        detail,
        status,
        status_word: word.to_owned(),
        running: agent.is_running(),
        mark,
        tool: match agent.origin {
            SubagentOrigin::Claude => ToolMark::Claude,
            SubagentOrigin::Codex => ToolMark::Codex,
            SubagentOrigin::Antigravity => ToolMark::Antigravity,
        },
        doing: read
            .filter(|_| agent.is_running())
            .map(|c| super::activity::doing(&c)),
    }
}

/// "71.1k in · 650 out · 68.2k cached (96%)": the cache share is of the input; left out with no
/// cache reads.
pub fn token_line(input: u64, output: u64, cached: u64) -> String {
    let mut line = format!("{} in · {} out", tokens(input), tokens(output));
    if cached > 0 && input > 0 {
        let share = (cached as f64 / input as f64 * 100.0).round().min(100.0);
        line.push_str(&format!(" · {} cached ({share}%)", tokens(cached)));
    }
    line
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
/// While any run, those are listed, then "Show N more" for the finished ones; `expanded` adds
/// them after the running ones. With none running, the `FINISHED_SHOWN` most recent finished
/// ones, then "Show N more"; `expanded` lists them all, then "Show less".
pub fn list_subagents(running: &[bool], expanded: bool) -> Listing {
    let newest_first = (0..running.len()).rev();
    if running.iter().any(|r| *r) {
        // The running ones lead; the finished ones wait behind "Show N more", never out of reach.
        let mut rows: Vec<usize> = newest_first.clone().filter(|i| running[*i]).collect();
        let finished: Vec<usize> = newest_first.filter(|i| !running[*i]).collect();
        if finished.is_empty() {
            return Listing {
                rows,
                hidden: 0,
                fold: Fold::None,
            };
        }
        if expanded {
            rows.extend(finished);
            return Listing {
                rows,
                hidden: 0,
                fold: Fold::Less,
            };
        }
        return Listing {
            rows,
            hidden: finished.len(),
            fold: Fold::More(finished.len()),
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
pub fn facts(conversation: &Conversation) -> Vec<(&'static str, String)> {
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
            token_line(f.input_tokens, f.output_tokens, f.cached_tokens),
        ));
    }
    if f.commands > 0 {
        out.push(("Commands", f.commands.to_string()));
    }
    // The files the session touched aren't listed: long paths, and the Git section says what changed.
    out
}

/// The inspector for a conversation. Shells are read-only here: running means a process still has
/// the output file open, which only the live app can ask (`writers`); a reader passes none, so a
/// shell with no notification shows as ended.
pub fn inspector(
    conversation: &Conversation,
    subagents: &[Subagent],
    writers: &dyn Fn(&str) -> Vec<i32>,
) -> Inspector {
    Inspector {
        tasks: task_rows(conversation),
        facts: facts(conversation),
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
                    output_path: shell.output_path.clone(),
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
        let i = inspector(&c, &[], &no_writers);
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
            origin: Default::default(),
        };
        let i = inspector(&Conversation::default(), &[agent], &no_writers);
        assert_eq!(
            i.subagents[0],
            SubagentRow {
                description: "Survey the repo".into(),
                detail: "Done · sonnet high · Explore".into(),
                status: Status::Done,
                status_word: "Done".into(),
                running: false,
                mark: Mark::Icon(Icon::Done, Ink::Done),
                tool: ToolMark::Claude,
                doing: None,
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
            origin: Default::default(),
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
            &no_writers,
        );
        assert_eq!(i.subagents[0].mark, Mark::Spinner);
        assert_eq!(i.subagents[0].detail, "Running");
        assert_eq!(i.subagents[1].detail, "Failed · Plan");
        assert_eq!(i.subagents_title(), "Subagents · 1 running");
    }

    #[test]
    fn a_job_handed_to_another_tool_names_the_tool_before_its_model() {
        let mut job = agent(SubagentStatus::Running, "Codex");
        job.origin = SubagentOrigin::Codex;
        job.model = Some("gpt-5.5".into());
        job.effort = Some("high".into());
        let mut gone = agent(SubagentStatus::Stopped, "Antigravity CLI");
        gone.origin = SubagentOrigin::Antigravity;
        let i = inspector(&Conversation::default(), &[job, gone], &no_writers);
        assert_eq!(i.subagents[0].detail, "Running · Codex · gpt-5.5 high");
        assert_eq!(i.subagents[0].doing, None, "no rollout yet, nothing to say");
        assert_eq!(i.subagents[1].detail, "Stopped · Antigravity CLI");
        assert_eq!(i.subagents[0].tool, ToolMark::Codex);
        assert_eq!(i.subagents[1].tool, ToolMark::Antigravity);
        assert_eq!(i.subagents_title(), "Subagents · 1 running");
    }

    #[test]
    fn a_running_codex_job_says_what_it_is_doing_from_its_rollout() {
        let s = crate::testutil::Scratch::new("codex-job-doing");
        let rollout = s.write(
            "rollout.jsonl",
            concat!(
                r#"{"timestamp":"2026-11-12T10:00:00.000Z","type":"session_meta","payload":{"id":"c1","cwd":"/w"}}"#,
                "\n",
                r#"{"timestamp":"2026-11-12T10:00:01.000Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"cargo test\"}","call_id":"k1"}}"#,
                "\n",
            ),
        );
        let mut job = agent(SubagentStatus::Running, "Codex");
        job.origin = SubagentOrigin::Codex;
        job.path = rollout.to_string_lossy().into_owned();
        let i = inspector(&Conversation::default(), &[job], &no_writers);
        let doing = i.subagents[0].doing.as_deref().expect("a running job says");
        assert!(doing.ends_with('…'), "{doing}");
        assert_ne!(doing, "Thinking…", "it is running a command");
    }

    #[test]
    fn a_running_subagent_says_what_it_is_doing_now() {
        let s = crate::testutil::Scratch::new("subagent-doing");
        let file = s.write(
            "agent-a.jsonl",
            concat!(
                r#"{"isSidechain":true,"type":"user","message":{"role":"user","content":"Fix it"},"uuid":"u1","timestamp":"2026-11-08T08:00:00.000Z"}"#,
                "\n",
                r#"{"isSidechain":true,"type":"assistant","message":{"id":"a1","role":"assistant","model":"claude-sonnet-5-5","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test","description":"Run the tests"}}],"usage":{"input_tokens":1000,"cache_read_input_tokens":231000,"output_tokens":300}},"uuid":"u2","timestamp":"2026-11-08T08:00:05.000Z"}"#,
                "\n"
            ),
        );
        let mut running = agent(SubagentStatus::Running, "general-purpose");
        running.path = file.to_string_lossy().into_owned();
        let mut done = agent(SubagentStatus::Completed, "general-purpose");
        done.path = running.path.clone();
        let i = inspector(&Conversation::default(), &[running, done], &no_writers);
        let doing = i.subagents[0].doing.as_deref().expect("a running one says");
        assert!(doing.contains("Run the tests"), "{doing}");
        assert_eq!(i.subagents[1].doing, None, "a finished one doesn't");
        // Both end with the context they hold, as a bare number, and lead with Claude's logo.
        assert_eq!(i.subagents[0].detail, "Running · 232.3k");
        assert_eq!(i.subagents[1].detail, "Done · 232.3k");
        assert_eq!(i.subagents[0].tool, ToolMark::Claude);
    }

    #[test]
    fn the_token_line_says_the_cached_part_and_its_share() {
        assert_eq!(token_line(71_100, 650, 0), "71.1k in · 650 out");
        assert_eq!(
            token_line(71_100, 650, 68_200),
            "71.1k in · 650 out · 68.2k cached (96%)"
        );
        assert_eq!(token_line(0, 10, 5), "0 in · 10 out");
    }

    #[test]
    fn the_tasks_bar_counts_done_of_all() {
        let row = |subject: &str, status| task_row(subject, status);
        assert_eq!(TaskSummary::of(&[]), None);
        let none_done =
            TaskSummary::of(&[row("a", TaskStatus::Pending), row("b", TaskStatus::Pending)])
                .unwrap();
        assert_eq!(none_done.title(), "Tasks 0/2");
        assert_eq!((none_done.fraction(), none_done.current), (0.0, None));
        let mid = TaskSummary::of(&[
            row("a", TaskStatus::Completed),
            row("b", TaskStatus::InProgress),
            row("c", TaskStatus::InProgress),
            row("d", TaskStatus::Pending),
        ])
        .unwrap();
        assert_eq!(mid.title(), "Tasks 1/4");
        assert_eq!(mid.fraction(), 0.25);
        assert_eq!(mid.current.as_deref(), Some("b"));
        assert!(!mid.all_done());
        let all = TaskSummary::of(&[row("a", TaskStatus::Completed)]).unwrap();
        assert!(all.all_done() && all.fraction() == 1.0);
    }

    #[test]
    fn titles_carry_the_counts() {
        assert_eq!(counted_title("Subagents", 2, 5), "Subagents · 2 running");
        assert_eq!(counted_title("Subagents", 0, 7), "Subagents · 7");
        assert_eq!(counted_title("Shells", 0, 0), "Shells");
        let mut c = Conversation::default();
        assert_eq!(inspector(&c, &[], &no_writers).tasks_title(), "Tasks");
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
        assert_eq!(inspector(&c, &[], &no_writers).tasks_title(), "Tasks · 1/2");
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
        let i = inspector(&c, &[], &writers);
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
    fn while_any_run_those_show_and_the_finished_ones_open_from_show_more() {
        let l = list_subagents(&[false, true, false, true, false], false);
        assert_eq!(l.rows, [3, 1], "newest first");
        assert_eq!((l.hidden, l.fold), (3, Fold::More(3)));
        // Opened: the running ones still first, then every finished one, newest first.
        let open = list_subagents(&[false, true, false, true, false], true);
        assert_eq!(open.rows, [3, 1, 4, 2, 0]);
        assert_eq!((open.hidden, open.fold), (0, Fold::Less));
        // All running: nothing to open.
        let all = list_subagents(&[true, true], false);
        assert_eq!((all.rows, all.fold), (vec![1, 0], Fold::None));
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
