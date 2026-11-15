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

const RAW_WORDS: [&str; 18] = [
    "push",
    "publish",
    "deploy",
    "release",
    "upload",
    "send-email",
    "apply",
    "destroy",
    "invoke",
    "mutation",
    "post",
    "put",
    "patch",
    "delete",
    "ssh",
    "scp",
    "sftp",
    "rsync",
];

pub fn is_outward(command: &str) -> bool {
    parsed_outward(command) || backstop(command)
}

pub(super) fn parsed_outward(command: &str) -> bool {
    command.len() > MAX_BYTES || script_outward(command, 0)
}

fn backstop(command: &str) -> bool {
    command
        .to_ascii_lowercase()
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .any(|word| RAW_WORDS.contains(&word))
}

#[derive(Default)]
struct Segment {
    words: Vec<String>,
    piped: bool,
    heredoc: Option<String>,
}

#[derive(Default)]
struct Split {
    segments: Vec<Segment>,
    nested: Vec<String>,
    uncertain: bool,
}

fn script_outward(script: &str, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return true;
    }
    let split = split(script);
    split.uncertain
        || split
            .nested
            .iter()
            .any(|inner| script_outward(inner, depth + 1))
        || split.segments.iter().any(|seg| segment_outward(seg, depth))
}

fn segment_outward(seg: &Segment, depth: usize) -> bool {
    let Some((program, args)) = program_of(&seg.words) else {
        return false;
    };
    let name = basename(program).to_ascii_lowercase();
    match name.as_str() {
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
    let mut stdin = false;
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        at += 1;
        match arg.as_str() {
            "--" => {
                if args.get(at).is_some() && !stdin {
                    return false;
                }
                break;
            }
            "-o" | "-O" | "+o" | "+O" | "--rcfile" | "--init-file" => at += 1,
            flag if flag.starts_with("--") || flag == "-" || flag.starts_with('+') => {}
            flag if flag.starts_with('-') => {
                if flag.contains('c') {
                    let script = args
                        .get(at)
                        .filter(|s| s.as_str() != "--")
                        .or(args.get(at + 1));
                    return script.is_some_and(|script| script_outward(script, depth + 1));
                }
                stdin |= flag.contains('s');
            }
            _ => {
                if !stdin {
                    return false;
                }
            }
        }
    }
    match &seg.heredoc {
        Some(body) => script_outward(body, depth + 1),
        None => seg.piped,
    }
}

fn basename(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
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

fn flag_script<'a>(words: &'a [String], names: &[&str], with_arg: &[&str]) -> Option<&'a [String]> {
    let mut at = 0;
    while let Some(word) = words.get(at) {
        if names.contains(&word.as_str()) {
            return words.get(at + 1..at + 2);
        }
        if with_arg.contains(&word.as_str()) {
            at += 2;
        } else if word.starts_with('-') || is_assignment(word) {
            at += 1;
        } else {
            break;
        }
    }
    None
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
            "env" => {
                if let Some(script) =
                    flag_script(after, &["-S", "--split-string"], &["-u", "-C", "-P"])
                {
                    return Some(("eval", script));
                }
                skip_flags(after, &["-u", "-C", "-P"])
            }
            "command" | "nohup" | "exec" | "builtin" => skip_flags(after, &[]),
            "nice" => skip_flags(after, &["-n"]),
            "time" => skip_flags(after, &["-f", "-o"]),
            "caffeinate" => skip_flags(after, &["-t", "-w"]),
            "stdbuf" => skip_flags(after, &["-i", "-o", "-e"]),
            "script" => {
                if let Some(script) = flag_script(after, &["-c", "--command"], &["-t", "-F"]) {
                    return Some(("eval", script));
                }
                skip_flags(after, &["-t"]).get(1..).unwrap_or(&[])
            }
            "timeout" => skip_flags(after, &["-s", "-k"]).get(1..).unwrap_or(&[]),
            "xargs" => skip_flags(
                after,
                &["-I", "-n", "-P", "-d", "-L", "-E", "-s", "-a", "-J"],
            ),
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
    here_next: bool,
    here: Option<String>,
    piped: bool,
    nested: Vec<String>,
    depth: i32,
    backtick: bool,
    uncertain: bool,
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
        if self.here_next {
            self.here_next = false;
            self.here = Some(word);
        } else if self.drop_next {
            self.drop_next = false;
        } else {
            self.words.push(word);
        }
    }

    fn end_segment(&mut self) {
        self.end_word();
        self.drop_next = false;
        self.here_next = false;
        if !self.words.is_empty() {
            self.segments.push(Segment {
                words: std::mem::take(&mut self.words),
                piped: self.piped,
                heredoc: self.here.take(),
            });
            self.piped = false;
        } else {
            self.here = None;
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
    expands: bool,
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
    let mut expands = true;
    while let Some(&c) = chars.get(at) {
        if c.is_whitespace() || ";|&()<>".contains(c) {
            break;
        }
        if matches!(c, '\'' | '"' | '\\') {
            expands = false;
        } else {
            delimiter.push(c);
        }
        at += 1;
    }
    (
        Heredoc {
            delimiter,
            strip_tabs,
            expands,
            segment,
        },
        at,
    )
}

fn take_body(chars: &[char], mut at: usize, doc: &Heredoc) -> (Vec<String>, usize, bool) {
    let mut body = Vec::new();
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
            return (body, at, true);
        }
        body.push(line.to_string());
    }
    (body, at, false)
}

fn read_bodies(
    chars: &[char],
    mut at: usize,
    out: &mut Splitter,
    pending: &mut Vec<Heredoc>,
) -> usize {
    for doc in pending.drain(..) {
        let (lines, next, found) = take_body(chars, at, &doc);
        at = next;
        out.uncertain |= !found || doc.delimiter.is_empty();
        let text = lines.join("\n");
        if doc.expands {
            scan_substitutions(&text, out);
        }
        if let Some(seg) = out.segments.get_mut(doc.segment) {
            seg.heredoc = Some(match seg.heredoc.take() {
                Some(before) => format!("{before}\n{text}"),
                None => text,
            });
        }
    }
    at
}

fn word_start(previous: Option<char>) -> bool {
    previous.is_none_or(|c| c.is_whitespace() || ";|&(<>".contains(c))
}

fn matching_paren(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 1;
    let mut pending: Vec<Heredoc> = Vec::new();
    let mut at = start;
    while at < chars.len() {
        let c = chars[at];
        let next = chars.get(at + 1).copied();
        match c {
            '\\' => at += 1,
            '\'' => {
                at += 1;
                while at < chars.len() && chars[at] != '\'' {
                    at += 1;
                }
            }
            '"' => {
                at += 1;
                while at < chars.len() && chars[at] != '"' {
                    at += usize::from(chars[at] == '\\') + 1;
                }
            }
            '$' if next == Some('\'') => {
                at += 2;
                while at < chars.len() && chars[at] != '\'' {
                    at += usize::from(chars[at] == '\\') + 1;
                }
            }
            '#' if word_start(at.checked_sub(1).map(|p| chars[p])) => {
                while at + 1 < chars.len() && chars[at + 1] != '\n' {
                    at += 1;
                }
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            '<' if next == Some('<') && chars.get(at + 2) != Some(&'<') => {
                let (doc, end) = heredoc_at(chars, at + 2, 0);
                pending.push(doc);
                at = end.max(at + 2) - 1;
            }
            '\n' if !pending.is_empty() => {
                for doc in pending.drain(..) {
                    at = take_body(chars, at + 1, &doc).1 - 1;
                }
            }
            _ => {}
        }
        at += 1;
    }
    None
}

fn scan_substitutions(text: &str, out: &mut Splitter) {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        match chars[at] {
            '\\' => at += 1,
            '$' if chars.get(at + 1) == Some(&'(') => match matching_paren(&chars, at + 2) {
                Some(end) => {
                    out.nested.push(chars[at + 2..end].iter().collect());
                    at = end;
                }
                None => {
                    out.uncertain = true;
                    return;
                }
            },
            '`' => match chars[at + 1..].iter().position(|&c| c == '`') {
                Some(p) => {
                    out.nested.push(chars[at + 1..at + 1 + p].iter().collect());
                    at += 1 + p;
                }
                None => {
                    out.uncertain = true;
                    return;
                }
            },
            _ => {}
        }
        at += 1;
    }
}

fn double_quoted(chars: &[char], mut at: usize, out: &mut Splitter) -> usize {
    out.in_word = true;
    out.quoted = true;
    while at < chars.len() && chars[at] != '"' {
        let c = chars[at];
        if c == '\\' && at + 1 < chars.len() {
            out.word.push(chars[at + 1]);
            at += 2;
        } else if c == '$' && chars.get(at + 1) == Some(&'(') {
            let Some(end) = matching_paren(chars, at + 2) else {
                out.uncertain = true;
                return chars.len();
            };
            out.nested.push(chars[at + 2..end].iter().collect());
            out.word.extend(chars[at..=end].iter());
            at = end + 1;
        } else if c == '`' {
            let Some(p) = chars[at + 1..].iter().position(|&x| x == '`') else {
                out.uncertain = true;
                return chars.len();
            };
            let end = at + 1 + p;
            out.nested.push(chars[at + 1..end].iter().collect());
            out.word.extend(chars[at..=end].iter());
            at = end + 1;
        } else {
            out.word.push(c);
            at += 1;
        }
    }
    out.uncertain |= at >= chars.len();
    at + 1
}

fn split(script: &str) -> Split {
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
                out.uncertain |= at >= chars.len();
                at += 1;
            }
            '$' if next == Some('\'') => {
                out.in_word = true;
                out.quoted = true;
                at += 1;
                while at < chars.len() && chars[at] != '\'' {
                    if chars[at] == '\\' && at + 1 < chars.len() {
                        at += 1;
                    }
                    out.word.push(chars[at]);
                    at += 1;
                }
                out.uncertain |= at >= chars.len();
                at += 1;
            }
            '$' if next == Some('(') && chars.get(at + 1) == Some(&'(') => {
                out.in_word = true;
                match matching_paren(&chars, at + 1) {
                    Some(end) => {
                        let inner: String = chars[at + 1..end].iter().collect();
                        if inner.chars().any(|c| c.is_ascii_alphabetic()) {
                            out.nested.push(inner);
                        }
                        at = end + 1;
                    }
                    None => {
                        out.uncertain = true;
                        at = chars.len();
                    }
                }
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
            '(' => {
                out.depth += 1;
                out.end_segment();
            }
            ')' => {
                out.depth -= 1;
                out.end_segment();
            }
            '`' => {
                out.backtick = !out.backtick;
                out.end_segment();
            }
            ';' => out.end_segment(),
            '|' => {
                out.end_segment();
                if next == Some('|') {
                    at += 1;
                } else {
                    out.piped = true;
                }
            }
            '&' => {
                if next == Some('&') {
                    at += 1;
                    out.end_segment();
                } else if next == Some('>') {
                    out.end_word();
                    out.push(c);
                } else {
                    out.end_segment();
                }
            }
            '<' if next == Some('<') && chars.get(at + 1) == Some(&'<') => {
                at += 2;
                out.end_word();
                out.here_next = true;
            }
            '<' if next == Some('<') => {
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
    out.uncertain |= out.depth != 0 || out.backtick || !pending.is_empty();
    Split {
        segments: out.segments,
        nested: out.nested,
        uncertain: out.uncertain,
    }
}
