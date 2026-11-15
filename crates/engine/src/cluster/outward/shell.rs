use super::commands;

const MAX_DEPTH: usize = 6;

const KEYWORDS: [&str; 11] = [
    "!", "{", "}", "if", "then", "else", "elif", "do", "while", "until", "time",
];

pub fn is_outward(command: &str) -> bool {
    script_outward(command, 0)
}

fn script_outward(script: &str, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return true;
    }
    if substitutions(script)
        .iter()
        .any(|inner| script_outward(inner, depth + 1))
    {
        return true;
    }
    split(script)
        .iter()
        .any(|words| segment_outward(words, depth))
}

fn segment_outward(words: &[String], depth: usize) -> bool {
    let words = without_redirects(words);
    let Some((program, args)) = program_of(&words) else {
        return false;
    };
    match basename(program) {
        "sh" | "bash" | "zsh" | "dash" => {
            inline_script(args).is_some_and(|script| script_outward(script, depth + 1))
        }
        "eval" => script_outward(&args.join(" "), depth + 1),
        name => commands::outward(name, args),
    }
}

fn basename(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

fn inline_script(args: &[String]) -> Option<&str> {
    let at = args.iter().position(|a| {
        a == "-c" || (a.starts_with('-') && !a.starts_with("--") && a.contains('c'))
    })?;
    args.get(at + 1).map(String::as_str)
}

fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn skip_flags<'a>(words: &'a [String], with_arg: &[&str]) -> &'a [String] {
    let mut at = 0;
    while let Some(word) = words.get(at) {
        if word == "--" {
            at += 1;
            break;
        }
        if !word.starts_with('-') || word == "-" {
            break;
        }
        at += if with_arg.contains(&word.as_str()) {
            2
        } else {
            1
        };
    }
    &words[at.min(words.len())..]
}

fn program_of(words: &[String]) -> Option<(&str, &[String])> {
    let mut rest = words;
    loop {
        let first = rest.first()?;
        let after = &rest[1..];
        if is_assignment(first) || KEYWORDS.contains(&first.as_str()) {
            rest = after;
            continue;
        }
        rest = match basename(first) {
            "sudo" | "doas" => skip_flags(
                after,
                &[
                    "-u", "-g", "-h", "-p", "-C", "-D", "-R", "-T", "-U", "-r", "-t",
                ],
            ),
            "env" => skip_flags(after, &["-u", "-C"]),
            "command" | "nohup" | "exec" | "builtin" => skip_flags(after, &[]),
            "nice" => skip_flags(after, &["-n"]),
            "timeout" => {
                let rest = skip_flags(after, &["-s", "-k"]);
                rest.get(1..).unwrap_or(&[])
            }
            "xargs" => skip_flags(after, &["-I", "-n", "-P", "-d", "-L", "-E", "-s", "-a"]),
            _ => return Some((first.as_str(), after)),
        };
    }
}

fn without_redirects(words: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(word) = words.get(at) {
        at += 1;
        let body = word.trim_start_matches(|c: char| c.is_ascii_digit());
        let body = body.strip_prefix('&').unwrap_or(body);
        if !(body.starts_with('>') || body.starts_with('<')) {
            out.push(word.clone());
            continue;
        }
        let target = body.trim_start_matches(['<', '>', '&', '|']);
        if target.is_empty() {
            at += 1;
        }
    }
    out
}

#[derive(Default)]
struct Splitter {
    segments: Vec<Vec<String>>,
    words: Vec<String>,
    word: String,
    in_word: bool,
}

impl Splitter {
    fn push(&mut self, c: char) {
        self.word.push(c);
        self.in_word = true;
    }

    fn end_word(&mut self) {
        if self.in_word {
            self.words.push(std::mem::take(&mut self.word));
            self.in_word = false;
        }
    }

    fn end_segment(&mut self) {
        self.end_word();
        if !self.words.is_empty() {
            self.segments.push(std::mem::take(&mut self.words));
        }
    }
}

struct Heredoc {
    delimiter: String,
    strip_tabs: bool,
}

fn heredoc_at(chars: &[char], mut at: usize) -> (Heredoc, usize) {
    let strip_tabs = chars.get(at) == Some(&'-');
    if strip_tabs {
        at += 1;
    }
    while matches!(chars.get(at), Some(' ' | '\t')) {
        at += 1;
    }
    let mut delimiter = String::new();
    while let Some(&c) = chars.get(at) {
        if c.is_whitespace() || ";|&()<>".contains(c) {
            break;
        }
        if !matches!(c, '\'' | '"' | '\\') {
            delimiter.push(c);
        }
        at += 1;
    }
    (
        Heredoc {
            delimiter,
            strip_tabs,
        },
        at,
    )
}

fn skip_bodies(chars: &[char], mut at: usize, pending: &mut Vec<Heredoc>) -> usize {
    for doc in pending.drain(..) {
        while at < chars.len() {
            let end = chars[at..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(chars.len(), |p| at + p);
            let line: String = chars[at..end].iter().collect();
            at = (end + 1).min(chars.len());
            let line = line.trim_end_matches('\r');
            let line = if doc.strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line
            };
            if line == doc.delimiter {
                break;
            }
        }
    }
    at
}

fn split(script: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = script.chars().collect();
    let mut out = Splitter::default();
    let mut pending: Vec<Heredoc> = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        let next = chars.get(at + 1).copied();
        at += 1;
        match c {
            '\'' => {
                out.in_word = true;
                while at < chars.len() && chars[at] != '\'' {
                    out.word.push(chars[at]);
                    at += 1;
                }
                at += 1;
            }
            '"' => {
                out.in_word = true;
                while at < chars.len() && chars[at] != '"' {
                    if chars[at] == '\\' && at + 1 < chars.len() {
                        at += 1;
                    }
                    out.word.push(chars[at]);
                    at += 1;
                }
                at += 1;
            }
            '\\' => {
                if let Some(escaped) = next {
                    at += 1;
                    if escaped != '\n' {
                        out.push(escaped);
                    }
                }
            }
            ' ' | '\t' | '\r' => out.end_word(),
            '\n' => {
                out.end_segment();
                if !pending.is_empty() {
                    at = skip_bodies(&chars, at, &mut pending);
                }
            }
            ';' | '|' | '(' | ')' | '`' => out.end_segment(),
            '&' => {
                let previous = at.checked_sub(2).and_then(|p| chars.get(p)).copied();
                if next == Some('&') {
                    at += 1;
                    out.end_segment();
                } else if matches!(previous, Some('<' | '>')) || next == Some('>') {
                    out.push(c);
                } else {
                    out.end_segment();
                }
            }
            '<' if next == Some('<') && chars.get(at + 1) != Some(&'<') => {
                let (doc, end) = heredoc_at(&chars, at + 1);
                pending.push(doc);
                at = end;
                out.end_word();
            }
            _ => out.push(c),
        }
    }
    out.end_segment();
    out.segments
}

fn substitutions(script: &str) -> Vec<String> {
    let chars: Vec<char> = script.chars().collect();
    let mut found = Vec::new();
    let (mut single, mut double) = (false, false);
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        match c {
            '\\' => at += 1,
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '$' if !single && chars.get(at + 1) == Some(&'(') => {
                let start = at + 2;
                let mut depth = 1;
                let mut end = start;
                while end < chars.len() {
                    match chars[end] {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        break;
                    }
                    end += 1;
                }
                found.push(chars[start..end.min(chars.len())].iter().collect());
                at = end;
            }
            '`' if !single => {
                let start = at + 1;
                let end = chars[start..]
                    .iter()
                    .position(|&c| c == '`')
                    .map_or(chars.len(), |p| start + p);
                found.push(chars[start..end].iter().collect());
                at = end;
            }
            _ => {}
        }
        at += 1;
    }
    found
}
