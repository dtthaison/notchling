use notchling_engine::transcript::model::{
    ChatItem, Conversation, DiffLine, ItemKind, ToolCall, ToolKind,
};
use notchling_kit::Icon;

use super::format::{duration, model_name};

pub const OUTPUT_LINES: usize = 12;

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    User {
        text: String,
        images: usize,
        queued: bool,
        sending: bool,
    },
    Assistant {
        text: String,
    },
    Thinking {
        title: String,
        text: String,
    },
    Tools(Vec<ToolRow>),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub text: String,
    pub more: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolRow {
    pub id: String,
    pub kind: ToolKind,
    pub icon: Icon,
    pub title: String,
    pub input: String,
    pub output: Option<Output>,
    pub diff: Vec<DiffLine>,
    pub added: usize,
    pub removed: usize,
    pub is_error: bool,
    pub finished: bool,
}

pub fn tool_icon(kind: ToolKind) -> Icon {
    match kind {
        ToolKind::Shell => Icon::Terminal,
        ToolKind::Read => Icon::File,
        ToolKind::Search => Icon::Search,
        ToolKind::Web => Icon::Web,
        ToolKind::Edit => Icon::Edit,
        ToolKind::Write => Icon::Write,
        ToolKind::Other => Icon::Plugin,
        ToolKind::Agent => Icon::Agent,
        ToolKind::Task => Icon::Task,
        ToolKind::Ask => Icon::Question,
        ToolKind::Skill => Icon::Skill,
    }
}

pub fn notice_icon(symbol: &str) -> Icon {
    if symbol.contains("exclamation") || symbol.contains("stop") || symbol.contains("xmark") {
        Icon::Needs
    } else if symbol.contains("checkmark") {
        Icon::Check
    } else {
        Icon::Info
    }
}

const TAB_WIDTH: usize = 4;

pub fn expand_tabs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut column = 0;
    for c in text.chars() {
        match c {
            '\t' => {
                let spaces = TAB_WIDTH - column % TAB_WIDTH;
                out.push_str(&" ".repeat(spaces));
                column += spaces;
            }
            '\n' | '\r' => {
                out.push(c);
                column = 0;
            }
            _ => {
                out.push(c);
                column += 1;
            }
        }
    }
    out
}

fn numbered(line: &str) -> Option<(&str, &str)> {
    let (number, code) = line.split_once('\t').unwrap_or((line, ""));
    let number = number.trim_start_matches(' ');
    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then_some((number, code))
}

fn listing(text: &str) -> Option<String> {
    let lines: Vec<(&str, &str)> = text.lines().map(numbered).collect::<Option<_>>()?;
    if !text.contains('\t') {
        return None;
    }
    let width = lines.iter().map(|(number, _)| number.len()).max()?;
    let drawn: Vec<String> = lines
        .iter()
        .map(|(number, code)| match expand_tabs(code).as_str() {
            "" => format!("{number:>width$}"),
            code => format!("{number:>width$}  {code}"),
        })
        .collect();
    Some(drawn.join("\n"))
}

pub fn cut_output(text: &str) -> Output {
    let ended = text.trim_end_matches(['\n', '\r']);
    let expanded = listing(ended).unwrap_or_else(|| expand_tabs(ended));
    let trimmed = expanded.trim_end_matches(['\n', '\r']);
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
        id: call.id.clone(),
        kind: call.kind,
        icon: tool_icon(call.kind),
        title: call.title.clone(),
        input: call.input.clone(),
        output: call
            .output
            .as_deref()
            .filter(|o| !o.trim().is_empty())
            .map(cut_output),
        diff: call
            .diff
            .iter()
            .map(|line| DiffLine {
                text: expand_tabs(&line.text),
                ..line.clone()
            })
            .collect(),
        added: call.added(),
        removed: call.removed(),
        is_error: call.is_error,
        finished: call.finished,
    }
}

pub fn row_of(item: &ChatItem) -> Row {
    match &item.kind {
        ItemKind::User { text, images } => Row::User {
            text: text.clone(),
            images: *images,
            queued: false,
            sending: false,
        },
        ItemKind::Assistant { text } => Row::Assistant { text: text.clone() },
        ItemKind::Thinking { seconds, text } => Row::Thinking {
            title: if *seconds >= 1.0 {
                format!("Thought for {}", duration(*seconds))
            } else {
                "Thought briefly".to_owned()
            },
            text: text.clone(),
        },
        ItemKind::Tools(calls) => Row::Tools(calls.iter().map(tool_row).collect()),
        ItemKind::TurnEnd { seconds, model } => Row::TurnEnd {
            text: match model {
                Some(model) => format!("Worked for {} · {}", duration(*seconds), model_name(model)),
                None => format!("Worked for {}", duration(*seconds)),
            },
        },
        ItemKind::Notice { text, symbol } => Row::Notice {
            text: text.clone(),
            icon: notice_icon(symbol),
        },
        ItemKind::CommandOutput { text } => Row::CommandOutput {
            text: expand_tabs(text),
        },
    }
}

pub fn rows(conversation: &Conversation) -> Vec<Row> {
    let mut out: Vec<Row> = conversation.items.iter().map(row_of).collect();
    out.extend(conversation.queued.iter().map(|text| Row::User {
        text: text.clone(),
        images: 0,
        queued: true,
        sending: false,
    }));
    out
}

pub fn with_held(mut rows: Vec<Row>, held: Option<&str>) -> Vec<Row> {
    rows.extend(held.map(|text| Row::User {
        text: text.to_owned(),
        images: 0,
        queued: true,
        sending: false,
    }));
    rows
}

pub fn with_outgoing(mut rows: Vec<Row>, outgoing: &[String]) -> Vec<Row> {
    rows.extend(outgoing.iter().map(|text| Row::User {
        text: text.clone(),
        images: 0,
        queued: true,
        sending: true,
    }));
    rows
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
                queued: false,
                sending: false
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
        assert_eq!(tools[0].id, "t1");
        assert_eq!(tools[0].kind, ToolKind::Shell);
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
                text: "Worked for 48s · Opus 5.5".into()
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
                queued: true,
                sending: false
            }
        );
    }

    #[test]
    fn messages_on_their_way_follow_as_bubbles_marked_sending() {
        let c = Conversation {
            queued: vec!["queued one".into()],
            ..Conversation::default()
        };
        let rows = with_outgoing(rows(&c), &["Fix the build".to_owned()]);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[1],
            Row::User {
                text: "Fix the build".into(),
                images: 0,
                queued: true,
                sending: true
            }
        );
    }

    #[test]
    fn a_thought_under_a_second_and_a_turn_without_a_model() {
        let c = Conversation {
            items: vec![
                item(ItemKind::Thinking {
                    seconds: 0.4,
                    text: String::new(),
                }),
                item(ItemKind::TurnEnd {
                    seconds: 205.0,
                    model: None,
                }),
            ],
            ..Conversation::default()
        };
        let rows = rows(&c);
        assert!(matches!(&rows[0], Row::Thinking { title, .. } if title == "Thought briefly"));
        assert_eq!(
            rows[1],
            Row::TurnEnd {
                text: "Worked for 3m 25s".into()
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

    const READ_OUTPUT: &str = "1\timport Foundation\n     2\t\n     3\tenum SessionSearch {\n     4\t    static func rank() {\n    10\t}\n   123\t\tlet tabbed = 1";

    #[test]
    fn a_reads_line_numbers_become_a_gutter_without_raw_tabs() {
        let out = cut_output(READ_OUTPUT);
        assert!(!out.text.contains('\t'), "{:?}", out.text);
        let lines: Vec<&str> = out.text.lines().collect();
        assert_eq!(lines[0], "  1  import Foundation");
        assert_eq!(lines[1], "  2");
        assert_eq!(lines[3], "  4      static func rank() {");
        assert_eq!(lines[4], " 10  }");
        assert_eq!(lines[5], "123      let tabbed = 1");
        let code_column = |line: &str, code: &str| line.find(code).unwrap();
        assert_eq!(
            code_column(lines[0], "import"),
            code_column(lines[2], "enum")
        );
        assert_eq!(code_column(lines[0], "import"), code_column(lines[4], "}"));
    }

    #[test]
    fn a_listing_whose_last_line_lost_its_tab_still_lines_up() {
        let out = cut_output("1\tfn main() {\n     2\t}\n     3");
        assert_eq!(out.text, "1  fn main() {\n2  }\n3");
    }

    #[test]
    fn output_that_is_not_a_numbered_listing_only_loses_its_tabs() {
        assert_eq!(cut_output("1\n2\n10").text, "1\n2\n10");
        assert_eq!(cut_output("1\ta\nplain\n2\tb").text, "1   a\nplain\n2   b");
        assert_eq!(cut_output("a\tb\nc").text, "a   b\nc");
    }

    #[test]
    fn tabs_expand_to_the_next_stop_of_four() {
        assert_eq!(expand_tabs("a\tb"), "a   b");
        assert_eq!(expand_tabs("ab\tc"), "ab  c");
        assert_eq!(expand_tabs("abcd\te"), "abcd    e");
        assert_eq!(expand_tabs("\t"), "    ");
        assert_eq!(expand_tabs("no tabs"), "no tabs");
        assert_eq!(expand_tabs("ab\tc\nd\te"), "ab  c\nd   e");
        assert_eq!(expand_tabs("é\tx"), "é   x");
    }

    #[test]
    fn a_diff_and_a_command_output_lose_their_raw_tabs_too() {
        let mut call = ToolCall::new(
            "t3",
            "Edit",
            ToolKind::Edit,
            "Edited main.go".into(),
            "/repo/main.go".into(),
        );
        call.diff = vec![DiffLine {
            kind: notchling_engine::transcript::model::DiffKind::Add,
            text: "\treturn nil".into(),
            number: Some(9),
        }];
        assert_eq!(tool_row(&call).diff[0].text, "    return nil");
        let c = Conversation {
            items: vec![item(ItemKind::CommandOutput {
                text: "a\tb".into(),
            })],
            ..Conversation::default()
        };
        assert_eq!(
            rows(&c),
            [Row::CommandOutput {
                text: "a   b".into()
            }]
        );
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
        assert_eq!(row.icon, Icon::Edit);
    }

    #[test]
    fn icons_by_kind_and_symbol() {
        assert_eq!(tool_icon(ToolKind::Agent), Icon::Agent);
        assert_eq!(tool_icon(ToolKind::Ask), Icon::Question);
        assert_eq!(tool_icon(ToolKind::Read), Icon::File);
        assert_eq!(tool_icon(ToolKind::Web), Icon::Web);
        assert_eq!(tool_icon(ToolKind::Write), Icon::Write);
        assert_eq!(tool_icon(ToolKind::Skill), Icon::Skill);
        assert_eq!(tool_icon(ToolKind::Other), Icon::Plugin);
        assert_eq!(notice_icon("rectangle.compress.vertical"), Icon::Info);
        assert_eq!(notice_icon("exclamationmark.triangle"), Icon::Needs);
        assert_eq!(notice_icon("checkmark.circle"), Icon::Check);
    }
}
