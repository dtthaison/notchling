//! A run of tool calls as the Swift app's `ToolStepView` folds it: one chip with the kinds' icons,
//! a summary ("Ran 4 commands, read 1 file"), how many failed, and whether one is still running.
//! Open, each call is a line of its own (`ToolCallRow`): what opens under it is `detail`. Pure, so
//! the wording is tested on every system.

use notchling_engine::transcript::model::ToolKind;
use notchling_kit::Icon;

use super::rows::ToolRow;

/// How many kind icons the chip stacks.
pub const CHIP_ICONS: usize = 4;

/// The verb and the noun's two forms for a kind (`ToolSummary.line` in the Swift app).
fn words(kind: ToolKind) -> (&'static str, &'static str, &'static str) {
    match kind {
        ToolKind::Shell => ("ran", "command", "commands"),
        ToolKind::Read => ("read", "file", "files"),
        ToolKind::Edit => ("edited", "file", "files"),
        ToolKind::Write => ("wrote", "file", "files"),
        ToolKind::Search => ("searched", "time", "times"),
        ToolKind::Web => ("browsed", "page", "pages"),
        ToolKind::Agent => ("ran", "agent", "agents"),
        ToolKind::Task => ("updated", "task", "tasks"),
        ToolKind::Ask => ("asked", "question", "questions"),
        ToolKind::Skill => ("used", "skill", "skills"),
        ToolKind::Other => ("used", "tool", "tools"),
    }
}

/// "Ran 2 commands, edited 3 files, searched 1 time", the kinds in the order they first appear.
/// Files are counted once each (the same file read twice is "read 1 file"). One call of a tool the
/// kinds don't name keeps its own title ("Used SendMessage"), since "used 1 tool" says nothing.
pub fn summary(steps: &[ToolRow]) -> String {
    let mut order: Vec<ToolKind> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    let mut files: Vec<Vec<&str>> = Vec::new();
    for step in steps {
        let at = match order.iter().position(|k| *k == step.kind) {
            Some(at) => at,
            None => {
                order.push(step.kind);
                counts.push(0);
                files.push(Vec::new());
                order.len() - 1
            }
        };
        counts[at] += 1;
        if matches!(step.kind, ToolKind::Read | ToolKind::Edit | ToolKind::Write)
            && !files[at].contains(&step.input.as_str())
        {
            files[at].push(&step.input);
        }
    }
    if let [step] = steps {
        if step.kind == ToolKind::Other {
            return step.title.clone();
        }
    }
    let text = order
        .iter()
        .enumerate()
        .map(|(at, kind)| {
            let (verb, one, many) = words(*kind);
            let n = if files[at].is_empty() {
                counts[at]
            } else {
                files[at].len()
            };
            format!("{verb} {n} {}", if n == 1 { one } else { many })
        })
        .collect::<Vec<_>>()
        .join(", ");
    notchling_kit::sentence(&text)
}

/// How many of the calls failed.
pub fn failed(steps: &[ToolRow]) -> usize {
    steps.iter().filter(|s| s.is_error).count()
}

/// "1 failed", "3 failed"; `None` when none did.
pub fn failed_text(steps: &[ToolRow]) -> Option<String> {
    match failed(steps) {
        0 => None,
        n => Some(format!("{n} failed")),
    }
}

/// A call is still running.
pub fn running(steps: &[ToolRow]) -> bool {
    steps.iter().any(|s| !s.finished)
}

/// The kinds' icons, each once, in order of first use, at most [`CHIP_ICONS`].
pub fn icons(steps: &[ToolRow]) -> Vec<Icon> {
    let mut out: Vec<Icon> = Vec::new();
    for step in steps {
        if !out.contains(&step.icon) {
            out.push(step.icon);
        }
        if out.len() == CHIP_ICONS {
            break;
        }
    }
    out
}

/// What opens under a step's line.
#[derive(Clone, Debug, PartialEq)]
pub enum Detail {
    /// Nothing: not clickable.
    None,
    /// A diff of the file; `path` is the input.
    Diff,
    /// The command or input in a box ("Command" for a shell, "Input" otherwise), then the output
    /// if any. A Read's input is its path, already in the title, so it shows only the output.
    Text {
        input_title: Option<&'static str>,
        has_output: bool,
    },
}

/// What opens under `step`, the Swift app's `hasDetail` rule: a diff, an output, or an input that
/// isn't a Read's path.
pub fn detail(step: &ToolRow) -> Detail {
    if !step.diff.is_empty() {
        return Detail::Diff;
    }
    let has_output = step.output.is_some();
    let input_title = (!step.input.is_empty() && step.kind != ToolKind::Read).then(|| {
        if step.kind == ToolKind::Shell {
            "Command"
        } else {
            "Input"
        }
    });
    if !has_output && input_title.is_none() {
        return Detail::None;
    }
    Detail::Text {
        input_title,
        has_output,
    }
}

/// The text of the input box under a step (a shell's command, any other call's input): the
/// string `step_detail` shows, when it shows one.
pub fn input_text(step: &ToolRow) -> Option<String> {
    match detail(step) {
        Detail::Text {
            input_title: Some(_),
            ..
        } => Some(step.input.clone()),
        _ => None,
    }
}

/// The text of the output box under a step: the output, with "… +n lines" when it was cut.
pub fn output_text(step: &ToolRow) -> Option<String> {
    match detail(step) {
        Detail::Text {
            has_output: true, ..
        } => step.output.as_ref().map(|output| {
            if output.more > 0 {
                format!("{}\n… +{} lines", output.text, output.more)
            } else {
                output.text.clone()
            }
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::rows::{tool_icon, Output};

    fn step(id: &str, kind: ToolKind, title: &str, input: &str) -> ToolRow {
        ToolRow {
            id: id.into(),
            kind,
            icon: tool_icon(kind),
            title: title.into(),
            input: input.into(),
            output: None,
            diff: Vec::new(),
            added: 0,
            removed: 0,
            is_error: false,
            finished: true,
        }
    }

    #[test]
    fn summaries_follow_the_swift_wording() {
        let shell = |id: &str| step(id, ToolKind::Shell, "Run it", "ls");
        assert_eq!(summary(&[shell("a")]), "Ran 1 command");
        assert_eq!(
            summary(&[shell("a"), shell("b"), shell("c"), shell("d")]),
            "Ran 4 commands"
        );
        assert_eq!(
            summary(&[step("r", ToolKind::Read, "Read a.rs", "/x/a.rs")]),
            "Read 1 file"
        );
        assert_eq!(
            summary(&[
                step("e1", ToolKind::Edit, "Edited a.rs", "/x/a.rs"),
                step("e2", ToolKind::Edit, "Edited b.rs", "/x/b.rs"),
            ]),
            "Edited 2 files"
        );
        // The same file twice is one file; a write is its own verb.
        assert_eq!(
            summary(&[
                step("e1", ToolKind::Edit, "Edited a.rs", "/x/a.rs"),
                step("e2", ToolKind::Edit, "Edited a.rs", "/x/a.rs"),
                step("w", ToolKind::Write, "Wrote c.rs", "/x/c.rs"),
            ]),
            "Edited 1 file, wrote 1 file"
        );
        // Kinds in order of first use, the first word capitalized.
        assert_eq!(
            summary(&[
                shell("a"),
                step("s", ToolKind::Search, "Searched for x", "x"),
                shell("b"),
                step("g", ToolKind::Agent, "Agent: review", "p"),
            ]),
            "Ran 2 commands, searched 1 time, ran 1 agent"
        );
        assert_eq!(
            summary(&[step("q", ToolKind::Ask, "Asked you: Which?", "")]),
            "Asked 1 question"
        );
        assert_eq!(
            summary(&[step("w", ToolKind::Web, "Read docs.rs", "https://docs.rs")]),
            "Browsed 1 page"
        );
        assert_eq!(
            summary(&[step("t", ToolKind::Task, "Updated the task list", "")]),
            "Updated 1 task"
        );
        assert_eq!(
            summary(&[step("k", ToolKind::Skill, "Used the x skill", "")]),
            "Used 1 skill"
        );
    }

    #[test]
    fn one_unnamed_tool_keeps_its_title_and_several_are_counted() {
        let send = step("o", ToolKind::Other, "Used SendMessage", "{}");
        assert_eq!(summary(std::slice::from_ref(&send)), "Used SendMessage");
        assert_eq!(summary(&[send.clone(), send]), "Used 2 tools");
        let shell = step("a", ToolKind::Shell, "Run it", "ls");
        assert_eq!(
            summary(&[shell, step("o", ToolKind::Other, "Used SendMessage", "{}")]),
            "Ran 1 command, used 1 tool"
        );
    }

    #[test]
    fn failures_running_and_icons() {
        let mut bad = step("b", ToolKind::Shell, "Run", "x");
        bad.is_error = true;
        let mut open = step("c", ToolKind::Read, "Read a", "/a");
        open.finished = false;
        let ok = step("a", ToolKind::Shell, "Run", "y");
        assert_eq!(failed_text(std::slice::from_ref(&ok)), None);
        assert_eq!(
            failed_text(&[ok.clone(), bad.clone()]).as_deref(),
            Some("1 failed")
        );
        assert_eq!(
            failed_text(&[bad.clone(), bad.clone(), bad.clone()]).as_deref(),
            Some("3 failed")
        );
        assert!(!running(&[ok.clone(), bad.clone()]));
        assert!(running(&[ok.clone(), open.clone()]));
        // Each icon once, in order, at most four.
        assert_eq!(
            icons(&[ok.clone(), bad, open.clone(), ok.clone()]),
            [Icon::Terminal, Icon::File]
        );
        let many = [
            ok,
            open,
            step("d", ToolKind::Edit, "Edited", "/b"),
            step("e", ToolKind::Search, "Searched", "q"),
            step("f", ToolKind::Web, "Read x", "u"),
        ];
        assert_eq!(icons(&many).len(), CHIP_ICONS);
    }

    #[test]
    fn what_opens_under_a_step() {
        let mut read = step("r", ToolKind::Read, "Read a.rs", "/x/a.rs");
        assert_eq!(detail(&read), Detail::None, "a path alone is in the title");
        read.output = Some(Output {
            text: "1  fn main() {}".into(),
            more: 0,
        });
        assert_eq!(
            detail(&read),
            Detail::Text {
                input_title: None,
                has_output: true
            }
        );
        let shell = step("s", ToolKind::Shell, "List", "ls");
        assert_eq!(
            detail(&shell),
            Detail::Text {
                input_title: Some("Command"),
                has_output: false
            }
        );
        let other = step("o", ToolKind::Other, "Used X", "{\"a\": 1}");
        assert_eq!(
            detail(&other),
            Detail::Text {
                input_title: Some("Input"),
                has_output: false
            }
        );
        assert_eq!(
            detail(&step("t", ToolKind::Task, "Updated the task list", "")),
            Detail::None
        );
        let mut edit = step("e", ToolKind::Edit, "Edited a.rs", "/x/a.rs");
        edit.diff = vec![notchling_engine::transcript::model::DiffLine {
            kind: notchling_engine::transcript::model::DiffKind::Add,
            text: "x".into(),
            number: Some(1),
        }];
        assert_eq!(detail(&edit), Detail::Diff);
    }
}
