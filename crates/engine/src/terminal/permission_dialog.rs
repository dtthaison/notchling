//! Reads Claude Code's permission dialog off a terminal screen ([`TerminalScreen::lines`]), so an
//! answer can be typed into a background job that no hook holds. The layout, as Claude Code draws
//! it at the bottom of the attach screen:
//!
//! ```text
//! ────────────────────────────────────────   (the dialog's top rule, from column 0)
//!  Bash command                              (the title: which tool)
//!
//!    rm -rf build                            (the body: the command, its description)
//!    Remove the build output
//!
//!  Do you want to proceed?                   (the question)
//!  ❯ 1. Yes
//!    2. Yes, and don't ask again for rm commands in /Users/me/api
//!    3. No, and tell Claude what to do differently (esc)
//!
//!  Esc to cancel                             (at most two rows after the options)
//! ```
//!
//! Older builds draw the same rows inside a box (`╭─╮`, `│ … │`); the borders are stripped. No
//! live recording of this dialog exists yet (docs/recording.md, `dialog-permission`): the tests
//! read made-up screens in `crates/engine/tests/fixtures/permission-made-up/`. Anything this
//! doesn't recognise reads as no dialog, so a plan built on it types nothing.
//!
//! [`TerminalScreen::lines`]: super::TerminalScreen::lines

use super::ask_dialog::{self, is_extra, option_row};

/// How far above the question the dialog's top rule is looked for.
const MAX_DIALOG_ROWS: usize = 40;
/// Drawn rows allowed under the last option (a footer, a hint): more means the options are text
/// higher up the screen, not the dialog waiting at its bottom.
const MAX_TRAILING_ROWS: usize = 2;

/// A permission answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// "Yes", this time.
    Allow,
    /// "Yes, and don't ask again …".
    Always,
    /// "No, and tell Claude what to do differently".
    Deny,
}

impl Decision {
    /// `allow`, `always` or `deny`, the local API's words.
    pub fn parse(word: &str) -> Option<Decision> {
        match word {
            "allow" => Some(Decision::Allow),
            "always" => Some(Decision::Always),
            "deny" => Some(Decision::Deny),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Always => "always",
            Decision::Deny => "deny",
        }
    }
}

/// What the card showed: the tool and the command or summary, from the job's state ("approve
/// Bash: ls -la"). The dialog on screen must be asking about this.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Expected {
    pub tool: String,
    pub summary: String,
}

/// One numbered option, as shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionOption {
    pub number: u64,
    pub label: String,
}

/// The permission dialog on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionDialog {
    /// "Bash command", "Edit file", "Fetch"…
    pub title: String,
    /// The rows between the title and the question: the command, its description, a diff.
    pub body: Vec<String>,
    /// "Do you want to proceed?"
    pub question: String,
    pub options: Vec<PermissionOption>,
}

impl PermissionDialog {
    /// The option that gives `decision`: "Yes" for Allow, the "don't ask again" one for Always,
    /// the "No" one for Deny. `None` when the dialog has no such option ("Yes, allow all edits
    /// during this session" is none of them).
    pub fn option_for(&self, decision: Decision) -> Option<&PermissionOption> {
        self.options.iter().find(|o| {
            let label = plain(&o.label);
            match decision {
                Decision::Allow => is_yes(&label),
                Decision::Always => is_always(&label),
                Decision::Deny => is_no(&label),
            }
        })
    }

    /// Whether this is the request the card showed: the same tool, and the summary's text in the
    /// dialog, compared without whitespace (the screen wraps a long command) and with an elided
    /// summary ("git log …") matched piece by piece. A file tool's path may be shown shorter on
    /// screen, so its file name is enough there.
    pub fn asks(&self, expected: &Expected) -> bool {
        self.shows_tool(&expected.tool) && self.shows_summary(&expected.tool, &expected.summary)
    }

    /// The title, body and question as one text.
    fn text(&self) -> String {
        let mut rows = vec![self.title.as_str()];
        rows.extend(self.body.iter().map(String::as_str));
        rows.push(&self.question);
        rows.join("\n")
    }

    fn shows_tool(&self, tool: &str) -> bool {
        let tool = tool.trim();
        if tool.is_empty() {
            return false;
        }
        let title = self.title.to_lowercase();
        let words: &[&str] = match tool {
            "Bash" => &["bash"],
            "Edit" | "MultiEdit" => &["edit"],
            "Write" => &["create", "write"],
            "Read" => &["read"],
            "NotebookEdit" => &["notebook"],
            "WebFetch" => &["fetch"],
            "WebSearch" => &["search"],
            _ => &[],
        };
        if !words.is_empty() {
            return words.iter().any(|w| title.contains(w));
        }
        // Any other tool (an MCP tool, Glob, Grep): its name, or an MCP tool's own part of
        // `mcp__server__tool`, somewhere in the dialog.
        let text = compact(&self.text()).to_lowercase();
        let short = tool.rsplit("__").next().unwrap_or(tool);
        text.contains(&compact(tool).to_lowercase())
            || (!short.is_empty() && text.contains(&compact(short).to_lowercase()))
    }

    fn shows_summary(&self, tool: &str, summary: &str) -> bool {
        let pieces: Vec<String> = summary
            .split('…')
            .flat_map(|p| p.split("..."))
            .map(compact)
            .filter(|p| !p.is_empty())
            .collect();
        if pieces.is_empty() {
            return true;
        }
        let text = compact(&self.text());
        if in_order(&text, &pieces) {
            return true;
        }
        let elided = pieces.len() > 1 || summary.contains('…') || summary.contains("...");
        if tool == "Bash" || elided || !summary.contains('/') {
            return false;
        }
        let name = summary.trim().rsplit('/').next().unwrap_or("");
        !name.is_empty() && text.contains(&compact(name))
    }
}

/// The permission dialog open at the bottom of the screen, or `None` (nothing open, a question,
/// or a layout this doesn't know).
pub fn read(lines: &[String]) -> Option<PermissionDialog> {
    // An AskUserQuestion is numbered too, and its question may well be "Do you want to …?".
    if ask_dialog::page(lines).is_some() {
        return None;
    }
    let drawn: Vec<&str> = lines
        .iter()
        .map(|l| l.trim_end())
        .filter(|l| !clean(l).is_empty())
        .collect();
    let q = drawn
        .iter()
        .rposition(|row| clean(row).starts_with("Do you want to"))?;
    let question = clean(drawn[q]).to_string();
    if !question.ends_with('?') {
        return None;
    }

    // The options right under the question, numbered from 1; an unnumbered row between two is
    // the wrapped end of the one above.
    let mut options: Vec<PermissionOption> = Vec::new();
    let mut loose: Vec<&str> = Vec::new();
    for row in &drawn[q + 1..] {
        let text = clean(row);
        match option_row(0, text) {
            Some(n) if n.number == options.len() as u64 + 1 => {
                if let Some(last) = options.last_mut() {
                    for more in loose.drain(..) {
                        last.label.push(' ');
                        last.label.push_str(more);
                    }
                }
                options.push(PermissionOption {
                    number: n.number,
                    label: n.label,
                });
            }
            Some(_) => return None,
            None if options.is_empty() => return None,
            // The input box: the dialog was text higher up, and the prompt waits under it.
            None if text.starts_with(['❯', '>']) || text.contains("? for shortcuts") => {
                return None
            }
            None if is_box_bottom(row) => {}
            None => loose.push(text),
        }
    }
    if options.len() < 2 || loose.len() > MAX_TRAILING_ROWS {
        return None;
    }
    if options.iter().any(|o| is_extra(&o.label)) {
        return None;
    }

    // Up to the dialog's top rule: the row under it is the title, the rest the body.
    let lowest = q.saturating_sub(MAX_DIALOG_ROWS);
    let top = (lowest..q).rev().find(|&i| is_top_rule(drawn[i]))?;
    let title_at = (top + 1..q).find(|&i| !clean(drawn[i]).is_empty())?;
    let title = clean(drawn[title_at]).to_string();
    let body = drawn[title_at + 1..q]
        .iter()
        .filter(|r| !is_rule(r))
        .map(|r| clean(r).to_string())
        .filter(|r| !r.is_empty())
        .collect();
    Some(PermissionDialog {
        title,
        body,
        question,
        options,
    })
}

/// A row with its outer box borders and spaces taken off.
fn clean(row: &str) -> &str {
    let row = row.trim();
    let row = row.strip_prefix('│').unwrap_or(row);
    let row = row.strip_suffix('│').unwrap_or(row);
    row.trim()
}

const RULE_CHARS: [char; 3] = ['─', '━', '═'];

/// A horizontal line: mostly rule characters (a label such as "─── record ─" may sit in it), or
/// a box's top or bottom.
fn is_rule(row: &str) -> bool {
    let row = row.trim();
    let rules = row.chars().filter(|c| RULE_CHARS.contains(c)).count();
    rules >= 8 && rules * 2 >= row.chars().count()
}

/// The bottom of a box drawn around the dialog.
fn is_box_bottom(row: &str) -> bool {
    matches!(row.trim().chars().next(), Some('╰' | '└')) && is_rule(row)
}

/// The dialog's top: a rule that starts at column 0 (a box inside the dialog, such as an edit's
/// diff, is indented), or the top of a box around the whole dialog.
fn is_top_rule(row: &str) -> bool {
    matches!(row.chars().next(), Some('─' | '━' | '═' | '╭' | '┌')) && is_rule(row)
}

/// Text without whitespace or box borders, so a wrapped row compares equal to one line.
fn compact(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '│')
        .collect()
}

/// Each piece in `text`, in order.
fn in_order(text: &str, pieces: &[String]) -> bool {
    let mut rest = text;
    for piece in pieces {
        match rest.find(piece.as_str()) {
            Some(at) => rest = &rest[at + piece.len()..],
            None => return false,
        }
    }
    true
}

/// A label lowercased, with a curly apostrophe made straight.
fn plain(label: &str) -> String {
    label.to_lowercase().replace('’', "'")
}

/// "Yes", or "Yes (y)", but not "Yes, and …" or "Yes, allow all …".
fn is_yes(label: &str) -> bool {
    label
        .strip_prefix("yes")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(" ("))
}

fn is_always(label: &str) -> bool {
    label.starts_with("yes") && label.contains("don't ask again")
}

/// "No", "No, and tell Claude what to do differently (esc)".
fn is_no(label: &str) -> bool {
    label
        .strip_prefix("no")
        .is_some_and(|rest| !rest.starts_with(char::is_alphanumeric))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> String {
        "─".repeat(120)
    }

    fn lines(rows: &[&str]) -> Vec<String> {
        rows.iter().map(|r| r.to_string()).collect()
    }

    fn bash(rule: &str) -> Vec<String> {
        lines(&[
            "❯ clean the build",
            rule,
            " Bash command",
            "",
            "   rm -rf build",
            "   Remove the build output",
            "",
            " Do you want to proceed?",
            " ❯ 1. Yes",
            "   2. Yes, and don't ask again for rm commands in /w/api",
            "   3. No, and tell Claude what to do differently (esc)",
            "",
            " Esc to cancel",
        ])
    }

    fn expected(tool: &str, summary: &str) -> Expected {
        Expected {
            tool: tool.into(),
            summary: summary.into(),
        }
    }

    #[test]
    fn reads_title_body_question_and_options() {
        let d = read(&bash(&rule())).expect("a dialog");
        assert_eq!(d.title, "Bash command");
        assert_eq!(d.body, ["rm -rf build", "Remove the build output"]);
        assert_eq!(d.question, "Do you want to proceed?");
        let labels: Vec<(u64, &str)> = d
            .options
            .iter()
            .map(|o| (o.number, o.label.as_str()))
            .collect();
        assert_eq!(
            labels,
            [
                (1, "Yes"),
                (2, "Yes, and don't ask again for rm commands in /w/api"),
                (3, "No, and tell Claude what to do differently (esc)"),
            ]
        );
        assert_eq!(d.option_for(Decision::Allow).unwrap().number, 1);
        assert_eq!(d.option_for(Decision::Always).unwrap().number, 2);
        assert_eq!(d.option_for(Decision::Deny).unwrap().number, 3);
    }

    #[test]
    fn matches_loosely_on_whitespace_and_elision() {
        let d = read(&bash(&rule())).unwrap();
        assert!(d.asks(&expected("Bash", "rm -rf build")));
        assert!(d.asks(&expected("Bash", "rm   -rf\nbuild")));
        assert!(d.asks(&expected("Bash", "rm -rf bu…")));
        assert!(
            d.asks(&expected("Bash", "")),
            "a tool alone, as Chat showed it"
        );
        assert!(!d.asks(&expected("Bash", "rm -rf dist")));
        assert!(!d.asks(&expected("Edit", "rm -rf build")));
        assert!(!d.asks(&expected("", "rm -rf build")));
    }

    #[test]
    fn a_question_or_a_prompt_is_no_permission_dialog() {
        let rule = rule();
        let ask = lines(&[
            &rule,
            "Do you want to deploy?",
            "❯ 1. Yes",
            "  2. No",
            "  3. Type something.",
            "  4. Chat about this",
            "Enter to select · Tab/Arrow keys to navigate · Esc to cancel",
        ]);
        assert_eq!(read(&ask), None);
        assert_eq!(read(&lines(&["❯ ", &rule, "  ? for shortcuts"])), None);
        // The dialog's words in what Claude wrote, with the prompt box under them.
        let mut said = bash(&rule);
        said.truncate(11);
        said.extend(lines(&[&rule, "❯ ", &rule, "  ? for shortcuts"]));
        assert_eq!(read(&said), None);
    }

    #[test]
    fn no_top_rule_reads_as_nothing() {
        let mut rows = bash("");
        rows.remove(1);
        assert_eq!(read(&rows), None);
    }

    #[test]
    fn options_must_count_from_one() {
        let mut rows = bash(&rule());
        rows[9] = "   3. Yes, and don't ask again".into();
        assert_eq!(read(&rows), None);
    }

    #[test]
    fn labels_say_which_answer_they_give() {
        assert!(is_yes("yes") && is_yes("yes (y)"));
        assert!(!is_yes("yes, and don't ask again") && !is_yes("yes, allow all edits"));
        assert!(is_always("yes, and don't ask again for rm commands"));
        assert!(!is_always(
            "yes, allow all edits during this session (shift+tab)"
        ));
        assert!(is_no("no") && is_no("no, and tell claude what to do differently (esc)"));
        assert!(!is_no("nothing") && !is_no("now"));
        assert_eq!(
            plain("Yes, and don’t ask again"),
            "yes, and don't ask again"
        );
    }

    #[test]
    fn decisions_read_the_local_apis_words() {
        for d in [Decision::Allow, Decision::Always, Decision::Deny] {
            assert_eq!(Decision::parse(d.word()), Some(d));
        }
        assert_eq!(Decision::parse("yes"), None);
    }
}
