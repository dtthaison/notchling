pub const COLLAPSED_LINES: usize = 4;
pub const EXPANDED_LINES: usize = 40;
const CHARS_PER_LINE: usize = 72;

pub const PREVIEW_DESCRIPTION: &str =
    "Run the migrations against the dev database and list the new tables";
pub const PREVIEW_COMMAND: &str = "cd services/api && sqlx migrate run --database-url postgres://localhost/api_dev --source ./migrations --ignore-missing && psql postgres://localhost/api_dev -c \"select table_name from information_schema.tables where table_schema = 'public' order by table_name\" | tee /tmp/api-tables.txt && wc -l /tmp/api-tables.txt && ls services/api/migrations";

pub fn headline(tool: &str, description: &str) -> String {
    let tool = tool.trim();
    let description = description.trim();
    match (tool, description.is_empty()) {
        ("Bash", false) => format!("Claude wants to run: {description}"),
        (_, false) => format!("Claude wants to use {tool}: {description}"),
        _ => format!("Claude wants to use {tool}"),
    }
}

pub fn description_shown<'a>(description: &'a str, summary: &str) -> &'a str {
    let description = description.trim();
    if description == summary.trim() {
        ""
    } else {
        description
    }
}

pub fn estimated_lines(text: &str) -> usize {
    text.trim_end()
        .split('\n')
        .map(|line| line.chars().count().div_ceil(CHARS_PER_LINE).max(1))
        .sum()
}

pub fn collapsible(text: &str) -> bool {
    estimated_lines(text) > COLLAPSED_LINES
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub max_lines: Option<usize>,
    pub fade: bool,
}

pub fn block(text: &str, open: bool) -> Block {
    if !collapsible(text) {
        return Block {
            max_lines: None,
            fade: false,
        };
    }
    if open {
        return Block {
            max_lines: Some(EXPANDED_LINES),
            fade: estimated_lines(text) > EXPANDED_LINES,
        };
    }
    Block {
        max_lines: Some(COLLAPSED_LINES),
        fade: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bash_description_leads_the_card() {
        assert_eq!(
            headline("Bash", " Commit the handoff doc "),
            "Claude wants to run: Commit the handoff doc"
        );
        assert_eq!(
            headline("Task", "Port the search"),
            "Claude wants to use Task: Port the search"
        );
        assert_eq!(headline("Bash", ""), "Claude wants to use Bash");
        assert_eq!(headline("Write", "  "), "Claude wants to use Write");
    }

    #[test]
    fn a_description_that_repeats_the_summary_is_dropped() {
        assert_eq!(description_shown("ls", "ls"), "");
        assert_eq!(description_shown(" List files ", "ls"), "List files");
        assert_eq!(description_shown("", "ls"), "");
    }

    #[test]
    fn lines_count_wraps_and_breaks() {
        assert_eq!(estimated_lines("ls"), 1);
        assert_eq!(estimated_lines(&"a".repeat(CHARS_PER_LINE)), 1);
        assert_eq!(estimated_lines(&"a".repeat(CHARS_PER_LINE + 1)), 2);
        assert_eq!(estimated_lines("a\n\nb\n"), 3);
        assert_eq!(estimated_lines(""), 1);
    }

    #[test]
    fn only_a_long_command_folds_and_it_opens_up_to_a_cap() {
        assert_eq!(
            block("ls -la", false),
            Block {
                max_lines: None,
                fade: false
            }
        );
        assert_eq!(
            block(PREVIEW_COMMAND, false),
            Block {
                max_lines: Some(COLLAPSED_LINES),
                fade: true
            }
        );
        assert_eq!(
            block(PREVIEW_COMMAND, true),
            Block {
                max_lines: Some(EXPANDED_LINES),
                fade: false
            }
        );
        let huge = "x\n".repeat(EXPANDED_LINES + 5);
        assert!(block(&huge, true).fade);
    }
}
