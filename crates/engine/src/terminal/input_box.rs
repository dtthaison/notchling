pub const RULE: char = '─';
pub const RULE_MIN: usize = 16;
pub const HOME_COL: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub held: Vec<String>,
    pub empty: bool,
}

pub fn is_rule(line: &str) -> bool {
    line.chars().take_while(|c| *c == RULE).count() >= RULE_MIN
}

pub fn span(lines: &[String]) -> Option<(usize, usize)> {
    let bottom = lines.iter().rposition(|l| is_rule(l))?;
    let top = lines[..bottom].iter().rposition(|l| is_rule(l))?;
    (bottom - top > 1).then_some((top, bottom))
}

fn bare(held: &[String]) -> bool {
    match held {
        [only] => only.chars().skip(1).all(char::is_whitespace),
        _ => false,
    }
}

pub fn read(lines: &[String], cursor: Option<(usize, usize)>) -> Option<Input> {
    let (top, bottom) = span(lines)?;
    let held: Vec<String> = lines[top + 1..bottom]
        .iter()
        .map(|l| l.trim_end().to_string())
        .collect();
    let home = cursor.is_some_and(|(row, col)| row == top + 1 && col <= HOME_COL);
    Some(Input {
        empty: home || bare(&held),
        held,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|l| l.to_string()).collect()
    }

    const RULE_LINE: &str = "────────────────────────────────";

    #[test]
    fn the_box_is_what_the_last_two_rules_hold() {
        let screen = lines(&[
            "⏺ ok",
            "",
            RULE_LINE,
            "❯ hello   ",
            "  there",
            RULE_LINE,
            "  ? for shortcuts",
        ]);
        assert_eq!(span(&screen), Some((2, 5)));
        let input = read(&screen, None).unwrap();
        assert_eq!(input.held, lines(&["❯ hello", "  there"]));
        assert!(!input.empty);
    }

    #[test]
    fn a_box_with_only_its_prompt_mark_is_empty() {
        for mark in ["❯ ", "❯", "! ", ""] {
            let screen = lines(&[RULE_LINE, mark, RULE_LINE]);
            assert!(read(&screen, None).unwrap().empty, "{mark:?}");
        }
        let typed = lines(&[RULE_LINE, "❯ x", RULE_LINE]);
        assert!(!read(&typed, None).unwrap().empty);
        let two = lines(&[RULE_LINE, "❯ ", "  x", RULE_LINE]);
        assert!(!read(&two, None).unwrap().empty);
    }

    #[test]
    fn a_hint_in_the_box_is_empty_while_the_cursor_sits_at_its_start() {
        let screen = lines(&[
            "⏺ ok",
            RULE_LINE,
            "❯ Try \"how do I log an error?\"",
            RULE_LINE,
        ]);
        assert!(
            !read(&screen, None).unwrap().empty,
            "words alone look typed"
        );
        assert!(read(&screen, Some((2, 2))).unwrap().empty);
        assert!(!read(&screen, Some((2, 30))).unwrap().empty);
        assert!(
            !read(&screen, Some((1, 2))).unwrap().empty,
            "another row's start"
        );
    }

    #[test]
    fn typed_text_is_never_empty_wherever_it_ends() {
        let screen = lines(&[RULE_LINE, "❯ one", "  two", "  three", RULE_LINE]);
        assert!(!read(&screen, Some((3, 7))).unwrap().empty);
        let shell = lines(&[RULE_LINE, "! ls -la", RULE_LINE, "  ! for shell mode"]);
        assert!(!read(&shell, Some((1, 8))).unwrap().empty);
    }

    #[test]
    fn a_named_session_draws_its_name_in_the_top_rule() {
        let top = format!("{RULE_LINE} menuprobe2 ─");
        let screen = lines(&[&top, "❯ ", RULE_LINE, "  status"]);
        assert!(read(&screen, Some((1, 2))).unwrap().empty);
    }

    #[test]
    fn one_rule_or_none_is_no_box() {
        assert_eq!(read(&lines(&["> ", "  ? for shortcuts"]), None), None);
        assert_eq!(
            read(
                &lines(&[RULE_LINE, " Bash command", " Do you want to proceed?"]),
                Some((1, 0))
            ),
            None
        );
        assert_eq!(read(&lines(&[RULE_LINE, RULE_LINE]), None), None);
        assert_eq!(read(&[], None), None);
    }

    #[test]
    fn a_short_or_indented_run_of_dashes_is_not_a_rule() {
        assert!(!is_rule("───────"));
        assert!(!is_rule("  ────────────────────────────────"));
        assert!(!is_rule("- - - - - - - - - - - - - - - - -"));
        assert!(is_rule(RULE_LINE));
    }
}
