//! `plain_blocks`: each row's selectable texts, in draw order, from the model alone.

use std::path::Path;

use notchling_engine::transcript::model::ToolKind;
use notchling_kit::Icon;

use super::blocks::{markdown_blocks, plain_blocks, Shown};
use super::markdown::Span;
use super::markdown_cache::{Inline, Item, Part};
use super::rows::{Output, Row, ToolRow};

fn words(text: &str) -> Vec<Span> {
    vec![Span {
        text: text.into(),
        ..Span::default()
    }]
}

fn inline(text: &str) -> Inline {
    Inline {
        spans: words(text),
        images: Vec::new(),
    }
}

fn user(text: &str) -> Row {
    Row::User {
        text: text.into(),
        images: 0,
        queued: false,
        sending: false,
    }
}

fn step(id: &str, kind: ToolKind, input: &str, output: Option<(&str, usize)>) -> ToolRow {
    ToolRow {
        id: id.into(),
        kind,
        icon: Icon::Terminal,
        title: "Ran it".into(),
        input: input.into(),
        output: output.map(|(text, more)| Output {
            text: text.into(),
            more,
        }),
        diff: Vec::new(),
        added: 0,
        removed: 0,
        is_error: false,
        finished: true,
    }
}

fn none_open(_: &str) -> bool {
    false
}

fn blocks_of(row: &Row, id: &str, open: &[&str], live: bool) -> Vec<String> {
    let is_open = |key: &str| open.contains(&key);
    let is_file = |p: &Path| p != Path::new("/tmp/gone.png");
    let shown = Shown {
        is_open: &is_open,
        live,
        is_file: &is_file,
    };
    plain_blocks(row, id, None, &shown)
}

#[test]
fn a_user_row_is_its_words_without_image_paths() {
    let row = user("look at these\n\n'/tmp/Shot 1.png' /tmp/a.rs /tmp/gone.png");
    assert_eq!(
        blocks_of(&row, "u1", &[], false),
        vec!["look at these\n\n/tmp/a.rs /tmp/gone.png".to_owned()]
    );
}

#[test]
fn a_picture_only_message_has_no_block() {
    assert!(blocks_of(&user("'/tmp/x y.png'"), "u1", &[], false).is_empty());
    // Words that aren't a picture are still drawn, even when they're all there is.
    assert_eq!(blocks_of(&user("hi"), "u1", &[], false), vec!["hi"]);
}

#[test]
fn markdown_parts_give_blocks_in_draw_order() {
    let message = vec![
        Part::Heading {
            level: 2,
            spans: words("Title"),
        },
        Part::Paragraph(inline("Some text")),
        Part::List(vec![
            Item {
                marker: "•".into(),
                depth: 0,
                inline: inline("one"),
            },
            Item {
                marker: "•".into(),
                depth: 0,
                inline: inline("two"),
            },
            Item {
                marker: "•".into(),
                depth: 1,
                inline: inline("three"),
            },
        ]),
        Part::Code {
            language: "rs".into(),
            text: "fn main() {}".into(),
        },
        Part::Table {
            header: vec![words("a"), words("b")],
            rows: vec![vec![words("1"), words("2")], vec![words("3"), words("4")]],
        },
        Part::Rule,
    ];
    let blocks = markdown_blocks(&message);
    assert_eq!(blocks.len(), 1 + 1 + 3 + 1 + (2 + 4));
    assert_eq!(
        blocks,
        [
            "Title",
            "Some text",
            "one",
            "two",
            "three",
            "fn main() {}",
            "a",
            "b",
            "1",
            "2",
            "3",
            "4"
        ]
    );
    // A reply row takes its blocks from the parsed message it is given.
    let is_file = |_: &Path| false;
    let shown = Shown {
        is_open: &none_open,
        live: false,
        is_file: &is_file,
    };
    let reply = Row::Assistant {
        text: "unused".into(),
    };
    assert_eq!(plain_blocks(&reply, "a1", Some(&message), &shown), blocks);
    assert!(plain_blocks(&reply, "a1", None, &shown).is_empty());
}

#[test]
fn a_run_of_spans_is_one_block() {
    let message = vec![Part::Paragraph(Inline {
        spans: vec![
            Span {
                text: "bold ".into(),
                bold: true,
                ..Span::default()
            },
            Span {
                text: "code".into(),
                code: true,
                ..Span::default()
            },
            Span {
                text: " link".into(),
                link: Some("https://example.com".into()),
                ..Span::default()
            },
        ],
        images: vec!["/tmp/x.png".into()],
    })];
    assert_eq!(markdown_blocks(&message), ["bold code link"]);
}

#[test]
fn a_closed_group_has_no_blocks_and_an_open_step_has_input_then_output() {
    let tools = Row::Tools(vec![
        step("s1", ToolKind::Shell, "ls", Some(("a\nb", 0))),
        step("s2", ToolKind::Shell, "pwd", Some(("/x", 0))),
    ]);
    assert!(blocks_of(&tools, "g1", &["s1"], false).is_empty());
    // The group open, no step: only the chip and the step lines, none selectable.
    assert!(blocks_of(&tools, "g1", &["g1"], false).is_empty());
    assert_eq!(
        blocks_of(&tools, "g1", &["g1", "s1"], false),
        ["ls", "a\nb"]
    );
    assert_eq!(
        blocks_of(&tools, "g1", &["g1", "s1", "s2"], false),
        ["ls", "a\nb", "pwd", "/x"]
    );
    // A Read's input is its path, in the title: only its output is a block.
    let read = Row::Tools(vec![step("r1", ToolKind::Read, "/x/a.rs", Some(("fn", 0)))]);
    assert_eq!(blocks_of(&read, "g2", &["g2", "r1"], false), ["fn"]);
    // A step with a diff shows lines that aren't selectable.
    let mut edit = step("e1", ToolKind::Edit, "/x/a.rs", None);
    edit.diff = vec![notchling_engine::transcript::model::DiffLine {
        kind: notchling_engine::transcript::model::DiffKind::Add,
        text: "x".into(),
        number: Some(1),
    }];
    assert!(blocks_of(&Row::Tools(vec![edit]), "g3", &["g3", "e1"], false).is_empty());
}

#[test]
fn the_live_group_counts_as_open() {
    let tools = Row::Tools(vec![step("s1", ToolKind::Shell, "ls", Some(("a", 0)))]);
    // Open by being live, but a step's detail still opens on its own click.
    assert!(blocks_of(&tools, "g1", &[], true).is_empty());
    assert_eq!(blocks_of(&tools, "g1", &["s1"], true), ["ls", "a"]);
}

#[test]
fn a_long_output_ends_with_its_more_lines_note() {
    let tools = Row::Tools(vec![step(
        "s1",
        ToolKind::Shell,
        "seq 1 99",
        Some(("1\n2", 97)),
    )]);
    assert_eq!(
        blocks_of(&tools, "g1", &["g1", "s1"], false),
        ["seq 1 99", "1\n2\n… +97 lines"]
    );
}

#[test]
fn a_command_output_is_its_text() {
    let row = Row::CommandOutput {
        text: "done".into(),
    };
    assert_eq!(blocks_of(&row, "c1", &[], false), ["done"]);
}

#[test]
fn thinking_turn_ends_and_notices_have_none() {
    let rows = [
        Row::Thinking {
            title: "Thought for 4s".into(),
            text: "hmm".into(),
        },
        Row::TurnEnd {
            text: "Worked for 48s".into(),
        },
        Row::Notice {
            text: "Switched account".into(),
            icon: Icon::Clock,
        },
    ];
    for row in &rows {
        assert!(blocks_of(row, "x", &["x"], true).is_empty());
    }
}
