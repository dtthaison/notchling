use super::model::{Outcome, OutcomeState, REASON_MAX};

const SECRET_LIKE: usize = 24;
const GAP: &str = "…";

fn hides(word: &str) -> bool {
    word.contains('/')
        || word.contains('\\')
        || word.starts_with('~')
        || (word.chars().count() >= SECRET_LIKE
            && word
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '=' | '+')))
}

fn without_controls(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_whitespace() || !c.is_control())
        .collect()
}

pub fn clean_reason(text: &str) -> Option<String> {
    let text = without_controls(text);
    let mut words: Vec<&str> = Vec::new();
    for word in text.split_whitespace() {
        let word = if hides(word) { GAP } else { word };
        if word == GAP && words.last() == Some(&GAP) {
            continue;
        }
        words.push(word);
    }
    let joined: String = words
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let joined = joined.trim();
    if joined.is_empty() || joined == GAP {
        return None;
    }
    let mut out: String = joined.chars().take(REASON_MAX).collect();
    if joined.chars().count() > REASON_MAX {
        out.pop();
        out.push('…');
    }
    Some(out)
}

fn names_a_path(word: &str) -> bool {
    word.starts_with('/')
        || word.starts_with('~')
        || word.contains('\\')
        || word.contains("/Users/")
        || word.contains("/home/")
}

pub fn clean_label(text: &str, max: usize) -> Option<String> {
    let text = without_controls(text);
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() || words.iter().any(|word| names_a_path(word)) {
        return None;
    }
    let joined = words.join(" ");
    let mut out: String = joined.chars().take(max).collect();
    if joined.chars().count() > max {
        out.pop();
        out.push('…');
    }
    Some(out)
}

pub fn outcome(state: OutcomeState, reason: Option<&str>, at_ms: i64) -> Outcome {
    Outcome {
        state,
        reason: reason.and_then(clean_reason),
        at_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_or_a_token_never_reaches_the_reason() {
        assert_eq!(
            clean_reason("Couldn't write ~/.claude/settings.json now."),
            Some("Couldn't write … now.".to_string())
        );
        assert_eq!(
            clean_reason("bad /Users/x/y and C:\\Users\\x then abcdefghijklmnopqrstuvwxyz0123"),
            Some("bad … and … then …".to_string())
        );
    }

    #[test]
    fn plain_words_pass_and_blank_text_is_none() {
        assert_eq!(
            clean_reason("The machine\nis   busy."),
            Some("The machine is busy.".to_string())
        );
        assert_eq!(clean_reason("  \n "), None);
        assert_eq!(clean_reason("/only/a/path"), None);
    }

    #[test]
    fn the_reason_is_cut_at_the_limit() {
        let long = "word ".repeat(200);
        let cleaned = clean_reason(&long).unwrap();
        assert_eq!(cleaned.chars().count(), REASON_MAX);
        assert!(cleaned.ends_with('…'));
    }

    #[test]
    fn an_outcome_carries_the_cleaned_reason() {
        let made = outcome(OutcomeState::Failed, Some("see /etc/hosts please"), 5);
        assert_eq!(made.reason.as_deref(), Some("see … please"));
        assert_eq!(made.at_ms, 5);
        assert_eq!(outcome(OutcomeState::Applied, None, 1).reason, None);
    }

    #[test]
    fn a_label_loses_control_characters_and_paths_but_keeps_a_branch_name() {
        assert_eq!(
            clean_label("fix/inbox\u{7}\n", 60),
            Some("fix/inbox".to_string())
        );
        assert_eq!(clean_label("/Users/x/repo", 60), None);
        assert_eq!(clean_label("~/repo", 60), None);
        assert_eq!(clean_label("C:\\Users\\x", 60), None);
        assert_eq!(clean_label("abcdef", 4), Some("abc…".to_string()));
    }

    #[test]
    fn a_control_character_inside_a_token_does_not_hide_it_from_redaction() {
        assert_eq!(
            clean_reason("key ghp_abcdefghij\u{7}klmnopqrstuvwxyz0123 end"),
            Some("key … end".to_string())
        );
        assert_eq!(
            clean_reason("key ghp_abcdefghij\u{1b}klmnopqrstuvwxyz0123"),
            Some("key …".to_string())
        );
    }

    #[test]
    fn one_path_among_the_words_drops_the_whole_label() {
        for text in [
            "repo /Users/alice/.ssh/id_rsa",
            "in ~/secret",
            "a C:\\x b",
            "see foo/home/x",
            "x/Users/y",
            "fix \u{7}/etc/passwd",
        ] {
            assert_eq!(clean_label(text, 60), None, "{text}");
        }
        assert_eq!(clean_label("fix/x", 60), Some("fix/x".to_string()));
        assert_eq!(
            clean_label("my  repo\tmain", 60),
            Some("my repo main".to_string())
        );
    }
}
