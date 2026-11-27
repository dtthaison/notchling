pub fn is_command(text: &str) -> bool {
    text.trim_start().starts_with('!')
}

pub fn normalize(text: &str) -> String {
    if !is_command(text) {
        return text.to_string();
    }
    let body = text.trim_start();
    let body = plain_characters(&body[1..].replace("\r\n", "\n"));
    let lines: Vec<&str> = body
        .split('\n')
        .filter(|l| !trim_spaces(l).starts_with("```"))
        .collect();
    let join_wrapped = !body.contains("<<");

    let mut result = String::new();
    for (index, raw) in lines.iter().enumerate() {
        if index == 0 || result.is_empty() {
            result = without_prompt(raw.trim_start()).to_string();
            continue;
        }
        let continues = raw.starts_with(char::is_whitespace) && !raw.trim().is_empty();
        if join_wrapped && continues && !result.ends_with('\\') {
            let rest = raw.trim_start();
            if result.ends_with(char::is_whitespace) {
                result.push_str(rest);
            } else if ends_with_separator(&result) {
                result.push(' ');
                result.push_str(rest);
            } else {
                result.push_str(rest);
            }
        } else {
            result.push('\n');
            result.push_str(without_prompt(raw));
        }
    }
    let tidy: Vec<&str> = result
        .split('\n')
        .map(|l| l.trim_end_matches([' ', '\t']))
        .collect();
    format!("!{}", tidy.join("\n").trim())
}

fn trim_spaces(line: &str) -> &str {
    line.trim_matches(|c: char| c.is_whitespace() && c != '\n' && c != '\r')
}

fn plain_characters(s: &str) -> String {
    let mut out = s.to_string();
    for (from, to) in [
        ("\u{201C}", "\""),
        ("\u{201D}", "\""),
        ("\u{201E}", "\""),
        ("\u{2018}", "'"),
        ("\u{2019}", "'"),
        ("\u{2014}", "--"),
        ("\u{2013}", "--"),
        ("\u{00A0}", " "),
        ("\u{2009}", " "),
        ("\u{202F}", " "),
        ("\u{200B}", ""),
    ] {
        out = out.replace(from, to);
    }
    out
}

fn without_prompt(line: &str) -> &str {
    ["$ ", "% "]
        .iter()
        .find_map(|p| line.strip_prefix(p))
        .unwrap_or(line)
}

fn ends_with_separator(text: &str) -> bool {
    text.split_whitespace()
        .last()
        .is_some_and(|last| ["then", "do", "else", "{", "(", "&&", "||", "|", ";"].contains(&last))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> String {
        normalize(s)
    }

    #[test]
    fn detection() {
        assert!(is_command("!git status"));
        assert!(is_command("  !ls"));
        assert!(!is_command("run git status!"));
        assert!(!is_command("/clear"));
        assert!(!is_command(""));
    }

    #[test]
    fn other_text_is_left_alone() {
        let text = "Please run:\n  git status\nand tell me";
        assert_eq!(n(text), text);
    }

    #[test]
    fn wrapped_in_the_middle_of_a_path() {
        assert_eq!(
            n("! nohup bash /private/tmp/claude-501/x/92318983-3f1e-4de7-916c-25\n  3a34ed5267/scratchpad/run.sh"),
            "!nohup bash /private/tmp/claude-501/x/92318983-3f1e-4de7-916c-253a34ed5267/scratchpad/run.sh"
        );
    }

    #[test]
    fn wrapped_at_a_space() {
        assert_eq!(
            n("! ssh root@1.2.3.4 'nohup apt-get -y install gitlab-runner </dev/null >/root/r.log \n  2>&1 &'"),
            "!ssh root@1.2.3.4 'nohup apt-get -y install gitlab-runner </dev/null >/root/r.log 2>&1 &'"
        );
    }

    #[test]
    fn wrapped_several_times() {
        assert_eq!(
            n("!7z x \"/Users/me/keys to Son.7z\" \n  -o\"/Users/me/go/src/ling\n  eva\""),
            "!7z x \"/Users/me/keys to Son.7z\" -o\"/Users/me/go/src/lingeva\""
        );
    }

    #[test]
    fn backslash_continuation_is_kept() {
        let text = "!docker run \\\n  -p 80:80 \\\n  nginx";
        assert_eq!(n(text), text);
    }

    #[test]
    fn separate_commands_stay_separate() {
        assert_eq!(n("!cd ~/go/src\nmake test"), "!cd ~/go/src\nmake test");
    }

    #[test]
    fn script_bodies_keep_their_words_apart() {
        assert_eq!(
            n("!for f in *.txt; do\n  echo $f\ndone"),
            "!for f in *.txt; do echo $f\ndone"
        );
        assert_eq!(n("!ls |\n  wc -l"), "!ls | wc -l");
    }

    #[test]
    fn heredocs_are_left_alone() {
        let text = "!cat <<EOF > a.txt\n  indented content\nEOF";
        assert_eq!(n(text), text);
    }

    #[test]
    fn prompts_fences_and_typography() {
        assert_eq!(n("!$ git status --short"), "!git status --short");
        assert_eq!(n("!```bash\ngit log -1\n```"), "!git log -1");
        assert_eq!(
            n("!git commit -m \u{201C}fix\u{201D} \u{2014}amend"),
            "!git commit -m \"fix\" --amend"
        );
        assert_eq!(n("!echo\u{00A0}hi   \n"), "!echo hi");
    }

    #[test]
    fn leading_spaces_and_blank_first_line() {
        assert_eq!(n("  !  git status"), "!git status");
        assert_eq!(n("!\n  git status"), "!git status");
    }

    #[test]
    fn running_twice_changes_nothing() {
        for text in [
            "!a\n  b",
            "!for f in x; do\n  echo $f\ndone",
            "!docker run \\\n  nginx",
            "!$ ls",
            "!cat <<EOF\n  x\nEOF",
        ] {
            assert_eq!(n(&n(text)), n(text), "{text}");
        }
    }
}
