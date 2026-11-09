//! Claude's Markdown as blocks (`MarkdownParser` in the Swift app): headings, paragraphs, lists,
//! quotes, rules, fenced code and tables, each paragraph's text then as spans of bold, italics,
//! `code`, strikethrough and links. No dependency: the subset Claude writes, parsed by hand and
//! tested on every system. An unclosed fence (a reply still streaming) runs to the end.

/// One block of a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading {
        level: usize,
        text: String,
    },
    Paragraph(String),
    List(Vec<ListItem>),
    Quote(String),
    Code {
        language: String,
        text: String,
    },
    Table {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Rule,
}

/// A list item: its marker as drawn ("•", or "1." as written), its text, how deep it nests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    pub marker: String,
    pub text: String,
    pub depth: usize,
}

/// A run of a paragraph's text in one style.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    /// Where a link goes.
    pub link: Option<String>,
}

/// The message as blocks.
pub fn blocks(text: &str) -> Vec<Block> {
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<Block> = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let mut list: Vec<ListItem> = Vec::new();
    let mut quote: Vec<String> = Vec::new();

    fn flush(
        out: &mut Vec<Block>,
        paragraph: &mut Vec<&str>,
        list: &mut Vec<ListItem>,
        quote: &mut Vec<String>,
    ) {
        if !paragraph.is_empty() {
            out.push(Block::Paragraph(paragraph.join("\n")));
            paragraph.clear();
        }
        if !list.is_empty() {
            out.push(Block::List(std::mem::take(list)));
        }
        if !quote.is_empty() {
            out.push(Block::Quote(quote.join("\n")));
            quote.clear();
        }
    }

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Fenced code.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush(&mut out, &mut paragraph, &mut list, &mut quote);
            let fence = &trimmed[..3];
            let language = trimmed[3..].trim().to_owned();
            let mut body: Vec<&str> = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with(fence) {
                body.push(lines[i]);
                i += 1;
            }
            out.push(Block::Code {
                language,
                text: body.join("\n"),
            });
            i += 1;
            continue;
        }

        // Tables: two or more lines with cells. The |---| row is optional; the first row is the
        // header either way.
        if is_table_line(trimmed) && i + 1 < lines.len() && is_table_line(lines[i + 1].trim()) {
            flush(&mut out, &mut paragraph, &mut list, &mut quote);
            let mut rows: Vec<Vec<String>> = Vec::new();
            let first = trimmed;
            while i < lines.len() && is_table_line(lines[i].trim()) {
                let row = lines[i].trim();
                if !is_separator_row(row) {
                    rows.push(cells(row));
                }
                i += 1;
            }
            // A copied table often drags in the end of the sentence before it: a header without
            // the leading pipe, one cell wider than every row below.
            if rows.len() > 1
                && !first.starts_with('|')
                && rows[0].len() > 1
                && rows[1..].iter().all(|r| r.len() == rows[0].len() - 1)
            {
                out.push(Block::Paragraph(rows[0][0].clone()));
                rows[0].remove(0);
            }
            if !rows.is_empty() {
                let width = rows.iter().map(Vec::len).max().unwrap_or(0);
                for row in &mut rows {
                    row.resize(width, String::new());
                }
                // A column that's empty everywhere (a stray trailing pipe) is dropped.
                let keep: Vec<usize> = (0..width)
                    .filter(|col| rows.iter().any(|r| !r[*col].is_empty()))
                    .collect();
                let mut kept: Vec<Vec<String>> = rows
                    .iter()
                    .map(|r| keep.iter().map(|c| r[*c].clone()).collect())
                    .collect();
                let header = kept.remove(0);
                out.push(Block::Table { header, rows: kept });
            }
            continue;
        }

        if trimmed.is_empty() {
            flush(&mut out, &mut paragraph, &mut list, &mut quote);
        } else if let Some((level, text)) = heading(trimmed) {
            flush(&mut out, &mut paragraph, &mut list, &mut quote);
            out.push(Block::Heading {
                level,
                text: text.to_owned(),
            });
        } else if is_rule(trimmed) {
            flush(&mut out, &mut paragraph, &mut list, &mut quote);
            out.push(Block::Rule);
        } else if let Some(rest) = trimmed.strip_prefix('>') {
            if !paragraph.is_empty() || !list.is_empty() {
                let q = std::mem::take(&mut quote);
                flush(&mut out, &mut paragraph, &mut list, &mut quote);
                quote = q;
            }
            quote.push(rest.trim().to_owned());
        } else if let Some(item) = list_item(line) {
            if !paragraph.is_empty() || !quote.is_empty() {
                let l = std::mem::take(&mut list);
                flush(&mut out, &mut paragraph, &mut list, &mut quote);
                list = l;
            }
            list.push(item);
        } else if !list.is_empty() && (line.starts_with("  ") || line.starts_with('\t')) {
            // A wrapped list item continues on an indented line.
            if let Some(last) = list.last_mut() {
                last.text.push(' ');
                last.text.push_str(trimmed);
            }
        } else {
            if !list.is_empty() || !quote.is_empty() {
                flush(&mut out, &mut paragraph, &mut list, &mut quote);
            }
            paragraph.push(line);
        }
        i += 1;
    }
    flush(&mut out, &mut paragraph, &mut list, &mut quote);
    out
}

/// `# Title` to `###### Title`: the level and the text.
fn heading(trimmed: &str) -> Option<(usize, &str)> {
    let level = trimmed.bytes().take_while(|b| *b == b'#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.starts_with(' ') {
        return None;
    }
    Some((level, rest.trim_start()))
}

fn is_rule(trimmed: &str) -> bool {
    trimmed.len() >= 3
        && ["-", "*", "_"]
            .iter()
            .any(|c| trimmed.chars().all(|ch| ch.to_string() == *c))
}

/// `- a`, `* a`, `+ a`, `• a`, `1. a`, `12) a`, with any indent.
fn list_item(line: &str) -> Option<ListItem> {
    let indent: usize = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum();
    let rest = line.trim_start_matches([' ', '\t']);
    let (marker, after) = if let Some(after) = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
        .or_else(|| rest.strip_prefix("• "))
    {
        ("•".to_owned(), after)
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 3 {
            return None;
        }
        let punct = rest.as_bytes().get(digits).copied();
        if punct != Some(b'.') && punct != Some(b')') {
            return None;
        }
        let after = rest.get(digits + 1..)?;
        if !after.starts_with(' ') && !after.starts_with('\t') {
            return None;
        }
        (rest[..=digits].to_owned(), after)
    };
    Some(ListItem {
        marker,
        text: after.trim_start().to_owned(),
        depth: (indent / 2).min(4),
    })
}

fn is_table_line(s: &str) -> bool {
    s.matches('|').count() >= 2 && !s.starts_with("```")
}

fn is_separator_row(s: &str) -> bool {
    s.contains('-') && s.chars().all(|c| "|-: ".contains(c))
}

fn cells(row: &str) -> Vec<String> {
    let mut parts: Vec<&str> = row.split('|').map(str::trim).collect();
    if row.starts_with('|') && !parts.is_empty() {
        parts.remove(0);
    }
    if row.ends_with('|') && !parts.is_empty() {
        parts.pop();
    }
    parts.into_iter().map(str::to_owned).collect()
}

/// The text's spans: `**bold**`, `*italics*` or `_italics_`, `` `code` ``, `~~struck~~`, and
/// `[text](url)` links. A marker with no closing twin is kept as written. Adjacent spans in the
/// same style are one span.
pub fn spans(text: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut current = Span::default();
    let mut bold = false;
    let mut italic = false;
    let mut strike = false;
    let bytes = text.as_bytes();
    let mut i = 0;

    fn push(out: &mut Vec<Span>, span: &mut Span) {
        if span.text.is_empty() {
            return;
        }
        if let Some(last) = out.last_mut() {
            let same = last.bold == span.bold
                && last.italic == span.italic
                && last.code == span.code
                && last.strike == span.strike
                && last.link == span.link;
            if same {
                last.text.push_str(&span.text);
                span.text.clear();
                return;
            }
        }
        out.push(std::mem::take(span));
    }
    let styled = |bold: bool, italic: bool, strike: bool| Span {
        bold,
        italic,
        strike,
        ..Span::default()
    };

    while i < bytes.len() {
        let rest = &text[i..];
        // Code: to the next backtick run of the same length; its text is taken as written.
        if rest.starts_with('`') {
            let ticks = rest.bytes().take_while(|b| *b == b'`').count();
            let fence = &rest[..ticks];
            if let Some(end) = rest[ticks..].find(fence) {
                push(&mut out, &mut current);
                let code = &rest[ticks..ticks + end];
                out.push(Span {
                    text: code.trim().to_owned(),
                    code: true,
                    ..Span::default()
                });
                current = styled(bold, italic, strike);
                i += ticks + end + ticks;
                continue;
            }
        }
        // A link: [text](url). Its text keeps the styles around it, and none inside.
        if rest.starts_with('[') {
            if let Some((label, url, len)) = link(rest) {
                push(&mut out, &mut current);
                out.push(Span {
                    text: label.to_owned(),
                    link: Some(url.to_owned()),
                    ..styled(bold, italic, strike)
                });
                current = styled(bold, italic, strike);
                i += len;
                continue;
            }
        }
        // Bold (`**` or `__`) and strikethrough (`~~`): opening needs a closing twin after it
        // (so the twin, when reached, closes).
        let double = [("**", true), ("__", true), ("~~", false)]
            .into_iter()
            .find(|(marker, _)| rest.starts_with(marker));
        if let Some((marker, is_bold)) = double {
            let on = if is_bold { bold } else { strike };
            let closes = on;
            let opens = !on && rest[2..].contains(marker) && !rest[2..].starts_with(' ');
            if closes || opens {
                push(&mut out, &mut current);
                if is_bold {
                    bold = !bold;
                } else {
                    strike = !strike;
                }
                current = styled(bold, italic, strike);
                i += 2;
                continue;
            }
        }
        if rest.starts_with('*') || rest.starts_with('_') {
            let marker = &rest[..1];
            let closes = italic;
            let before_ok = marker == "*" || i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let opens = !italic
                && before_ok
                && rest[1..].contains(marker)
                && !rest[1..].starts_with(' ')
                && !rest[1..].starts_with(marker);
            if closes || opens {
                push(&mut out, &mut current);
                italic = !italic;
                current = styled(bold, italic, strike);
                i += 1;
                continue;
            }
        }
        let c = rest.chars().next().unwrap_or(' ');
        current.text.push(c);
        i += c.len_utf8();
    }
    // Styles left open had no closing twin after all: their text stays, as written.
    push(&mut out, &mut current);
    out
}

/// `[label](url)` at the start of `rest`: the label, the url, how many bytes it took.
fn link(rest: &str) -> Option<(&str, &str, usize)> {
    let close = rest.find("](")?;
    let label = &rest[1..close];
    if label.contains('[') || label.contains('\n') {
        return None;
    }
    let after = &rest[close + 2..];
    let end = after.find(')')?;
    let url = after[..end].trim();
    if url.is_empty() || url.contains(' ') {
        return None;
    }
    Some((label, url, close + 2 + end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(marker: &str, text: &str, depth: usize) -> ListItem {
        ListItem {
            marker: marker.into(),
            text: text.into(),
            depth,
        }
    }

    #[test]
    fn paragraphs_headings_lists_and_rules() {
        let text = "## What failed\n\nOne test failed.\nIt expects 7.\n\n- fmt\n- clippy\n  with warnings\n\n1. Fix it\n2) Run again\n\n---\n\n> A quote\n> continues\n\nEnd.";
        assert_eq!(
            blocks(text),
            [
                Block::Heading {
                    level: 2,
                    text: "What failed".into()
                },
                Block::Paragraph("One test failed.\nIt expects 7.".into()),
                Block::List(vec![
                    item("•", "fmt", 0),
                    item("•", "clippy with warnings", 0)
                ]),
                Block::List(vec![item("1.", "Fix it", 0), item("2)", "Run again", 0)]),
                Block::Rule,
                Block::Quote("A quote\ncontinues".into()),
                Block::Paragraph("End.".into()),
            ]
        );
        // Nested items keep their depth; a `#` without a space is text.
        assert_eq!(
            blocks("- a\n  - b\n    - c\n#tag"),
            [
                Block::List(vec![
                    item("•", "a", 0),
                    item("•", "b", 1),
                    item("•", "c", 2)
                ]),
                Block::Paragraph("#tag".into()),
            ]
        );
        // A list right after a paragraph ends it.
        assert_eq!(
            blocks("Steps:\n- one\n- two"),
            [
                Block::Paragraph("Steps:".into()),
                Block::List(vec![item("•", "one", 0), item("•", "two", 0)]),
            ]
        );
    }

    #[test]
    fn fenced_code_keeps_its_text_and_an_open_fence_runs_to_the_end() {
        assert_eq!(
            blocks("Run:\n\n```sh\nls -la\n\necho \"**not bold**\"\n```\nDone."),
            [
                Block::Paragraph("Run:".into()),
                Block::Code {
                    language: "sh".into(),
                    text: "ls -la\n\necho \"**not bold**\"".into()
                },
                Block::Paragraph("Done.".into()),
            ]
        );
        assert_eq!(
            blocks("```rust\nlet x = 1;"),
            [Block::Code {
                language: "rust".into(),
                text: "let x = 1;".into()
            }]
        );
        assert_eq!(
            blocks("~~~\nplain\n~~~"),
            [Block::Code {
                language: String::new(),
                text: "plain".into()
            }]
        );
    }

    #[test]
    fn tables_with_and_without_a_separator_row() {
        let with = "| Check | Result |\n| --- | --- |\n| fmt | passed |\n| tests | 1 failed |";
        let table = Block::Table {
            header: vec!["Check".into(), "Result".into()],
            rows: vec![
                vec!["fmt".into(), "passed".into()],
                vec!["tests".into(), "1 failed".into()],
            ],
        };
        assert_eq!(blocks(with), std::slice::from_ref(&table));
        assert_eq!(
            blocks("| Check | Result |\n| fmt | passed |\n| tests | 1 failed |"),
            [table]
        );
        // A sentence's end dragged into the header comes out before the table.
        assert_eq!(
            blocks("like | A | B |\n| 1 | 2 |\n| 3 | 4 |"),
            [
                Block::Paragraph("like".into()),
                Block::Table {
                    header: vec!["A".into(), "B".into()],
                    rows: vec![vec!["1".into(), "2".into()], vec!["3".into(), "4".into()]],
                }
            ]
        );
        // A short row is padded.
        assert_eq!(
            blocks("| A | B |\n| 1 | 2 |\n| 3 |"),
            [Block::Table {
                header: vec!["A".into(), "B".into()],
                rows: vec![vec!["1".into(), "2".into()], vec!["3".into(), "".into()]],
            }]
        );
        // One line with pipes is a paragraph.
        assert_eq!(blocks("a | b | c"), [Block::Paragraph("a | b | c".into())]);
    }

    fn plain(text: &str) -> Span {
        Span {
            text: text.to_owned(),
            ..Span::default()
        }
    }

    #[test]
    fn inline_styles() {
        assert_eq!(
            spans("One **bold** and *italic* and `code` and ~~gone~~."),
            [
                plain("One "),
                Span {
                    text: "bold".into(),
                    bold: true,
                    ..Span::default()
                },
                plain(" and "),
                Span {
                    text: "italic".into(),
                    italic: true,
                    ..Span::default()
                },
                plain(" and "),
                Span {
                    text: "code".into(),
                    code: true,
                    ..Span::default()
                },
                plain(" and "),
                Span {
                    text: "gone".into(),
                    strike: true,
                    ..Span::default()
                },
                plain("."),
            ]
        );
        // Bold around code, italics with underscores, bold italics.
        assert_eq!(
            spans("**`name`** in _rows.rs_ is ***both***"),
            [
                Span {
                    text: "name".into(),
                    code: true,
                    ..Span::default()
                },
                plain(" in "),
                Span {
                    text: "rows.rs".into(),
                    italic: true,
                    ..Span::default()
                },
                plain(" is "),
                Span {
                    text: "both".into(),
                    bold: true,
                    italic: true,
                    ..Span::default()
                },
            ]
        );
    }

    #[test]
    fn markers_without_a_twin_stay_as_written() {
        assert_eq!(spans("2 * 3 = 6"), [plain("2 * 3 = 6")]);
        assert_eq!(spans("a ** b"), [plain("a ** b")]);
        assert_eq!(
            spans("snake_case_name here"),
            [plain("snake_case_name here")]
        );
        assert_eq!(spans("`unclosed code"), [plain("`unclosed code")]);
        assert_eq!(spans("**unclosed bold"), [plain("**unclosed bold")]);
        // Markdown inside code is code.
        assert_eq!(
            spans("`a **b** c`"),
            [Span {
                text: "a **b** c".into(),
                code: true,
                ..Span::default()
            }]
        );
    }

    #[test]
    fn links() {
        assert_eq!(
            spans("See [the rules](https://example.com/rules.md) and [x]( not a link )."),
            [
                plain("See "),
                Span {
                    text: "the rules".into(),
                    link: Some("https://example.com/rules.md".into()),
                    ..Span::default()
                },
                plain(" and [x]( not a link )."),
            ]
        );
        // A link inside bold keeps the bold; a bare bracket is text.
        assert_eq!(
            spans("**[a](b)** [c"),
            [
                Span {
                    text: "a".into(),
                    bold: true,
                    link: Some("b".into()),
                    ..Span::default()
                },
                plain(" [c"),
            ]
        );
    }

    #[test]
    fn spans_join_back_into_the_text_without_markers() {
        let text = "Mixed **bold** `code` and _it_ plus [l](u) end";
        let joined: String = spans(text).iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "Mixed bold code and it plus l end");
        // Multi-byte text is kept whole.
        assert_eq!(
            spans("é **ü** ∿"),
            [
                plain("é "),
                Span {
                    text: "ü".into(),
                    bold: true,
                    ..Span::default()
                },
                plain(" ∿")
            ]
        );
    }
}
