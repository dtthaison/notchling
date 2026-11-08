//! The conversation as rows to draw: the engine's items, plus the queued messages as bubbles at the
//! end. Pure, so the mapping is tested on every system.

use notchling_engine::transcript::model::{
    ChatItem, Conversation, DiffLine, ItemKind, ToolCall, ToolKind,
};
use notchling_kit::Icon;

use super::format::{duration, model_name};

/// How many lines of a tool's output a row shows before "… +N lines".
pub const OUTPUT_LINES: usize = 12;

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// Your message; `queued` while it waits for the turn to end.
    User {
        text: String,
        images: usize,
        queued: bool,
    },
    Assistant {
        text: String,
    },
    Thinking {
        /// "Thought for 4s".
        title: String,
        text: String,
    },
    Tools(Vec<ToolRow>),
    /// "48s · Opus 5.5".
    TurnEnd {
        text: String,
    },
    Notice {
        text: String,
        icon: Icon,
    },
    CommandOutput {
        text: String,
    },
}

/// A tool's output, cut to [`OUTPUT_LINES`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub text: String,
    /// Lines left out.
    pub more: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolRow {
    pub icon: Icon,
    pub title: String,
    /// The command, path, query or input.
    pub input: String,
    pub output: Option<Output>,
    pub diff: Vec<DiffLine>,
    pub added: usize,
    pub removed: usize,
    pub is_error: bool,
    pub finished: bool,
}

/// The icon for a tool's kind.
pub fn tool_icon(kind: ToolKind) -> Icon {
    match kind {
        ToolKind::Shell => Icon::Terminal,
        ToolKind::Read | ToolKind::Search | ToolKind::Web => Icon::Search,
        ToolKind::Edit | ToolKind::Write | ToolKind::Other => Icon::Tools,
        ToolKind::Agent | ToolKind::Task => Icon::Sessions,
        ToolKind::Ask => Icon::Question,
        ToolKind::Skill => Icon::Claude,
    }
}

/// The kit icon for a notice's SF Symbol name (the Swift app's), by what the symbol means.
pub fn notice_icon(symbol: &str) -> Icon {
    if symbol.contains("exclamation") || symbol.contains("stop") || symbol.contains("xmark") {
        Icon::Needs
    } else if symbol.contains("checkmark") {
        Icon::Check
    } else {
        Icon::Info
    }
}

/// The first [`OUTPUT_LINES`] lines, and how many follow.
pub fn cut_output(text: &str) -> Output {
    let trimmed = text.trim_end_matches(['\n', '\r']);
    let total = trimmed.lines().count();
    if total <= OUTPUT_LINES {
        return Output {
            text: trimmed.to_owned(),
            more: 0,
        };
    }
    Output {
        text: trimmed
            .lines()
            .take(OUTPUT_LINES)
            .collect::<Vec<_>>()
            .join("\n"),
        more: total - OUTPUT_LINES,
    }
}

fn tool_row(call: &ToolCall) -> ToolRow {
    ToolRow {
        icon: tool_icon(call.kind),
        title: call.title.clone(),
        input: call.input.clone(),
        output: call
            .output
            .as_deref()
            .filter(|o| !o.trim().is_empty())
            .map(cut_output),
        diff: call.diff.clone(),
        added: call.added(),
        removed: call.removed(),
        is_error: call.is_error,
        finished: call.finished,
    }
}

fn row(item: &ChatItem) -> Row {
    match &item.kind {
        ItemKind::User { text, images } => Row::User {
            text: text.clone(),
            images: *images,
            queued: false,
        },
        ItemKind::Assistant { text } => Row::Assistant { text: text.clone() },
        ItemKind::Thinking { seconds, text } => Row::Thinking {
            title: format!("Thought for {}", duration(*seconds)),
            text: text.clone(),
        },
        ItemKind::Tools(calls) => Row::Tools(calls.iter().map(tool_row).collect()),
        ItemKind::TurnEnd { seconds, model } => Row::TurnEnd {
            text: match model {
                Some(model) => format!("{} · {}", duration(*seconds), model_name(model)),
                None => duration(*seconds),
            },
        },
        ItemKind::Notice { text, symbol } => Row::Notice {
            text: text.clone(),
            icon: notice_icon(symbol),
        },
        ItemKind::CommandOutput { text } => Row::CommandOutput { text: text.clone() },
    }
}

/// Every item as a row, then the queued messages as your bubbles marked queued.
pub fn rows(conversation: &Conversation) -> Vec<Row> {
    let mut out: Vec<Row> = conversation.items.iter().map(row).collect();
    out.extend(conversation.queued.iter().map(|text| Row::User {
        text: text.clone(),
        images: 0,
        queued: true,
    }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::transcript::model::DiffKind;

    fn item(kind: ItemKind) -> ChatItem {
        ChatItem {
            id: "i".into(),
            kind,
            date: None,
        }
    }

    #[test]
    fn items_become_rows_and_queued_messages_follow() {
        let mut call = ToolCall::new(
            "t1",
            "Bash",
            ToolKind::Shell,
            "List files".into(),
            "ls".into(),
        );
        call.output = Some("a\nb\n".into());
        call.finished = true;
        let c = Conversation {
            items: vec![
                item(ItemKind::user("Fix it")),
                item(ItemKind::Thinking {
                    seconds: 4.0,
                    text: "Hmm".into(),
                }),
                item(ItemKind::Tools(vec![call])),
                item(ItemKind::Assistant {
                    text: "Done.".into(),
                }),
                item(ItemKind::TurnEnd {
                    seconds: 48.0,
                    model: Some("claude-opus-5-5".into()),
                }),
                item(ItemKind::notice("Interrupted", "stop.circle")),
                item(ItemKind::CommandOutput {
                    text: "Total cost: $1".into(),
                }),
            ],
            queued: vec!["And bump the version".into()],
            ..Conversation::default()
        };
        let rows = rows(&c);
        assert_eq!(rows.len(), 8);
        assert_eq!(
            rows[0],
            Row::User {
                text: "Fix it".into(),
                images: 0,
                queued: false
            }
        );
        assert_eq!(
            rows[1],
            Row::Thinking {
                title: "Thought for 4s".into(),
                text: "Hmm".into()
            }
        );
        let Row::Tools(tools) = &rows[2] else {
            panic!("tools");
        };
        assert_eq!(tools[0].icon, Icon::Terminal);
        assert_eq!(tools[0].title, "List files");
        assert_eq!(
            tools[0].output,
            Some(Output {
                text: "a\nb".into(),
                more: 0
            })
        );
        assert_eq!(
            rows[4],
            Row::TurnEnd {
                text: "48s · Opus 5.5".into()
            }
        );
        assert_eq!(
            rows[5],
            Row::Notice {
                text: "Interrupted".into(),
                icon: Icon::Needs
            }
        );
        assert_eq!(
            rows[7],
            Row::User {
                text: "And bump the version".into(),
                images: 0,
                queued: true
            }
        );
    }

    #[test]
    fn long_output_is_cut_with_a_count() {
        let text = (1..=20)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let out = cut_output(&text);
        assert_eq!(out.more, 8);
        assert_eq!(out.text.lines().count(), OUTPUT_LINES);
        assert_eq!(cut_output("one\n\n").text, "one");
    }

    #[test]
    fn edits_carry_their_diff_counts_and_no_output() {
        let mut call = ToolCall::new(
            "t2",
            "Edit",
            ToolKind::Edit,
            "Edited a.txt".into(),
            "/repo/a.txt".into(),
        );
        call.diff = vec![
            DiffLine {
                kind: DiffKind::Remove,
                text: "old".into(),
                number: Some(3),
            },
            DiffLine {
                kind: DiffKind::Add,
                text: "new".into(),
                number: Some(3),
            },
        ];
        call.output = Some("   ".into());
        let row = tool_row(&call);
        assert_eq!((row.added, row.removed), (1, 1));
        assert_eq!(row.output, None, "blank output is no output");
        assert_eq!(row.icon, Icon::Tools);
    }

    #[test]
    fn icons_by_kind_and_symbol() {
        assert_eq!(tool_icon(ToolKind::Agent), Icon::Sessions);
        assert_eq!(tool_icon(ToolKind::Ask), Icon::Question);
        assert_eq!(tool_icon(ToolKind::Read), Icon::Search);
        assert_eq!(notice_icon("rectangle.compress.vertical"), Icon::Info);
        assert_eq!(notice_icon("exclamationmark.triangle"), Icon::Needs);
        assert_eq!(notice_icon("checkmark.circle"), Icon::Check);
    }
}
