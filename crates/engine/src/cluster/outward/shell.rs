use super::commands;

const MAX_DEPTH: usize = 6;
const MAX_BYTES: usize = 65_536;

const KEYWORDS: [&str; 10] = [
    "!", "{", "}", "if", "then", "else", "elif", "do", "while", "until",
];

const PACKAGE_FLAGS: [&str; 7] = [
    "-C",
    "--prefix",
    "-w",
    "--workspace",
    "--filter",
    "-F",
    "--cwd",
];

#[derive(Default)]
struct Segment {
    words: Vec<String>,
    piped: bool,
    heredoc: Option<String>,
}

pub fn is_outward(command: &str) -> bool {
    command.len() > MAX_BYTES || script_outward(command, 0)
}

fn script_outward(script: &str, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return true;
    }
    let (segments, nested) = split(script);
    nested.iter().any(|inner| script_outward(inner, depth + 1))
        || segments.iter().any(|seg| segment_outward(seg, depth))
}

fn segment_outward(seg: &Segment, depth: usize) -> bool {
    let Some((program, args)) = program_of(&seg.words) else {
        return false;
    };
    match basename(program) {
        "sh" | "bash" | "zsh" | "dash" => shell_outward(args, seg, depth),
        "eval" => script_outward(&args.join(" "), depth + 1),
        "watch" => script_outward(
            &skip_flags(args, &["-n", "--interval"]).join(" "),
            depth + 1,
        ),
        name => commands::outward(name, args),
    }
}

fn shell_outward(args: &[String], seg: &Segment, depth: usize) -> bool {
    if let Some(script) = inline_script(args) {
        return script_outward(script, depth + 1);
    }
    if args.iter().any(|a| !a.starts_with('-')) {
        return false;
    }
    match &seg.heredoc {
        Some(body) => script_outward(body, depth + 1),
        None => seg.piped,
    }
}

fn basename(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

fn inline_script(args: &[String]) -> Option<&str> {
    let at = args.iter().position(|a| {
        a == "-c" || (a.starts_with('-') && !a.starts_with("--") && a.contains('c'))
    })?;
    let script = args.get(at + 1)?;
    if script == "--" {
        return args.get(at + 2).map(String::as_str);
    }
    Some(script)
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

fn runs_package(subcommand: &str) -> bool {
    matches!(subcommand, "dlx" | "exec" | "x")
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
            "time" => skip_flags(after, &["-f", "-o"]),
            "caffeinate" => skip_flags(after, &["-t", "-w"]),
            "stdbuf" => skip_flags(after, &["-i", "-o", "-e"]),
            "script" => skip_flags(after, &["-t"]).get(1..).unwrap_or(&[]),
            "timeout" => skip_flags(after, &["-s", "-k"]).get(1..).unwrap_or(&[]),
            "xargs" => skip_flags(after, &["-I", "-n", "-P", "-d", "-L", "-E", "-s", "-a"]),
            "npx" | "bunx" | "pnpx" => skip_flags(after, &["-p", "--package"]),
            "npm" | "pnpm" | "yarn" | "bun" => {
                let inner = skip_flags(after, &PACKAGE_FLAGS);
                match inner.split_first() {
                    Some((sub, tail)) if runs_package(sub) => {
                        skip_flags(tail, &["-p", "--package"])
                    }
                    _ => return Some((first.as_str(), after)),
                }
            }
            _ => return Some((first.as_str(), after)),
        };
    }
}

#[derive(Default)]
struct Splitter {
    segments: Vec<Segment>,
    words: Vec<String>,
    word: String,
    in_word: bool,
    quoted: bool,
    drop_next: bool,
    piped: bool,
    nested: Vec<String>,
}

impl Splitter {
    fn push(&mut self, c: char) {
        self.word.push(c);
        self.in_word = true;
    }

    fn end_word(&mut self) {
        if !self.in_word {
            return;
        }
        let word = std::mem::take(&mut self.word);
        self.in_word = false;
        self.quoted = false;
        if self.drop_next {
            self.drop_next = false;
        } else {
            self.words.push(word);
        }
    }

    fn end_segment(&mut self) {
        self.end_word();
        self.drop_next = false;
        if !self.words.is_empty() {
            self.segments.push(Segment {
                words: std::mem::take(&mut self.words),
                piped: self.piped,
                heredoc: None,
            });
            self.piped = false;
        }
    }

    fn is_fd(&self) -> bool {
        !self.quoted
            && ((!self.word.is_empty() && self.word.chars().all(|c| c.is_ascii_digit()))
                || self.word == "&")
    }
}

struct Heredoc {
    delimiter: String,
    strip_tabs: bool,
    segment: usize,
}

fn heredoc_at(chars: &[char], mut at: usize, segment: usize) -> (Heredoc, usize) {
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
            segment,
        },
        at,
    )
}

fn read_bodies(
    chars: &[char],
    mut at: usize,
    out: &mut Splitter,
    pending: &mut Vec<Heredoc>,
) -> usize {
    for doc in pending.drain(..) {
        let mut body: Vec<String> = Vec::new();
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
            body.push(line.to_string());
        }
        if let Some(seg) = out.segments.get_mut(doc.segment) {
            let text = body.join("\n");
            seg.heredoc = Some(match seg.heredoc.take() {
                Some(before) => format!("{before}\n{text}"),
                None => text,
            });
        }
    }
    at
}

fn matching_paren(chars: &[char], start: usize) -> usize {
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
    end.min(chars.len())
}

fn double_quoted(chars: &[char], mut at: usize, out: &mut Splitter) -> usize {
    out.in_word = true;
    out.quoted = true;
    while at < chars.len() && chars[at] != '"' {
        let c = chars[at];
        if c == '\\' && at + 1 < chars.len() {
            at += 1;
            out.word.push(chars[at]);
            at += 1;
        } else if c == '$' && chars.get(at + 1) == Some(&'(') {
            let end = matching_paren(chars, at + 2);
            out.nested.push(chars[at + 2..end].iter().collect());
            out.word
                .extend(chars[at..(end + 1).min(chars.len())].iter());
            at = end + 1;
        } else if c == '`' {
            let end = chars[at + 1..]
                .iter()
                .position(|&x| x == '`')
                .map_or(chars.len(), |p| at + 1 + p);
            out.nested.push(chars[at + 1..end].iter().collect());
            out.word
                .extend(chars[at..(end + 1).min(chars.len())].iter());
            at = end + 1;
        } else {
            out.word.push(c);
            at += 1;
        }
    }
    at + 1
}

fn split(script: &str) -> (Vec<Segment>, Vec<String>) {
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
                out.quoted = true;
                while at < chars.len() && chars[at] != '\'' {
                    out.word.push(chars[at]);
                    at += 1;
                }
                at += 1;
            }
            '"' => at = double_quoted(&chars, at, &mut out),
            '\\' => {
                if let Some(escaped) = next {
                    at += 1;
                    if escaped != '\n' {
                        out.push(escaped);
                    }
                }
            }
            '#' if !out.in_word => {
                while at < chars.len() && chars[at] != '\n' {
                    at += 1;
                }
            }
            ' ' | '\t' | '\r' => out.end_word(),
            '\n' => {
                out.end_segment();
                if !pending.is_empty() {
                    at = read_bodies(&chars, at, &mut out, &mut pending);
                }
            }
            ';' | '(' | ')' | '`' => out.end_segment(),
            '|' => {
                out.end_segment();
                if next == Some('|') {
                    at += 1;
                } else {
                    out.piped = true;
                }
            }
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
                let (doc, end) = heredoc_at(&chars, at + 1, out.segments.len());
                pending.push(doc);
                at = end;
                out.end_word();
            }
            '<' | '>' => {
                if out.is_fd() {
                    out.word.clear();
                    out.in_word = false;
                } else {
                    out.end_word();
                }
                while matches!(chars.get(at), Some('<' | '>' | '&')) {
                    at += 1;
                }
                out.drop_next = true;
            }
            _ => out.push(c),
        }
    }
    out.end_segment();
    (out.segments, out.nested)
}
