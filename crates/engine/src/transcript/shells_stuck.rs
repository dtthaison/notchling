use regex::Regex;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

pub const STUCK_AFTER_S: u64 = 30 * 60;

const VALUE_FLAGS: [char; 10] = ['F', 'G', 'P', 'U', 'd', 'g', 's', 't', 'u', 'O'];

const SCOPING_FLAGS: [char; 6] = ['F', 'P', 'g', 's', 't', 'v'];

const SCOPING_LONG: [&str; 6] = [
    "--parent",
    "--pidfile",
    "--pgroup",
    "--session",
    "--terminal",
    "--inverse",
];

const VALUE_LONG: [&str; 14] = [
    "--parent",
    "--pidfile",
    "--pgroup",
    "--session",
    "--terminal",
    "--uid",
    "--euid",
    "--group",
    "--delimiter",
    "--older",
    "--ns",
    "--nslist",
    "--runstates",
    "--cgroup",
];

const AFTER_LOOP: [&str; 9] = [
    "echo",
    "printf",
    "true",
    "exit",
    "say",
    "afplay",
    "touch",
    "notify-send",
    "terminal-notifier",
];

const CACHE_MAX: usize = 512;

static SHAPES: LazyLock<Mutex<HashMap<String, Shape>>> = LazyLock::new(Mutex::default);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stuck {
    Itself,
    LongLoop,
}

impl Stuck {
    pub fn code(self) -> &'static str {
        match self {
            Stuck::Itself => "matches_itself",
            Stuck::LongLoop => "long_wait_loop",
        }
    }
}

pub fn running_secs(started: Option<f64>, now: f64) -> Option<u64> {
    let started = started?;
    (now.is_finite() && started.is_finite() && started > 0.0)
        .then(|| (now - started).max(0.0) as u64)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Shape {
    pub itself: bool,
    pub wait_loop: bool,
}

fn cached(command: &str) -> Option<Shape> {
    let cache = SHAPES.lock().unwrap_or_else(PoisonError::into_inner);
    cache.get(command).copied()
}

pub fn shape_of(command: &str) -> Shape {
    if let Some(shape) = cached(command) {
        return shape;
    }
    let shape = Shape {
        itself: matches_itself(command),
        wait_loop: is_wait_loop(command),
    };
    let mut cache = SHAPES.lock().unwrap_or_else(PoisonError::into_inner);
    if cache.len() >= CACHE_MAX {
        cache.clear();
    }
    cache.insert(command.to_owned(), shape);
    shape
}

pub fn stuck_by(shape: Shape, running_s: Option<u64>) -> Option<Stuck> {
    if shape.itself {
        return Some(Stuck::Itself);
    }
    (shape.wait_loop && running_s.is_some_and(|s| s > STUCK_AFTER_S)).then_some(Stuck::LongLoop)
}

pub fn looks_stuck(command: &str, running_s: Option<u64>) -> Option<Stuck> {
    stuck_by(shape_of(command), running_s)
}

fn word_at(text: &str, at: usize, word: &str) -> bool {
    let bytes = text.as_bytes();
    let end = at + word.len();
    let name = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.';
    text[at..].starts_with(word)
        && (at == 0 || !name(bytes[at - 1]))
        && (end >= bytes.len() || !name(bytes[end]))
}

fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word)
        .any(|(at, _)| word_at(text, at, word))
}

fn ends_with_word(text: &str, word: &str) -> bool {
    text.len() >= word.len() && word_at(text, text.len() - word.len(), word)
}

fn runs_after_the_loop(command: &str) -> bool {
    let Some(end) = command
        .match_indices("done")
        .filter(|(at, _)| word_at(command, *at, "done"))
        .map(|(at, _)| at + "done".len())
        .last()
    else {
        return false;
    };
    let tail =
        command[end..].trim_start_matches([' ', '\t', '\n', ';', '&', '|', ')', '\'', '"', '}']);
    let first: String = tail
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, ';' | '&' | '|' | ')' | '\'' | '"'))
        .collect();
    !first.is_empty() && !AFTER_LOOP.contains(&first.as_str())
}

pub fn is_wait_loop(command: &str) -> bool {
    (has_word(command, "while") || has_word(command, "until"))
        && has_word(command, "pgrep")
        && has_word(command, "sleep")
        && !runs_after_the_loop(command)
}

fn waits_while_present(command: &str, at: usize) -> bool {
    let before = command[..at].trim_end();
    let (negated, before) = match before.strip_suffix('!') {
        Some(rest) => (true, rest.trim_end()),
        None => (false, before),
    };
    if ends_with_word(before, "while") {
        !negated
    } else if ends_with_word(before, "until") {
        negated
    } else {
        false
    }
}

struct Word {
    text: String,
    expands: bool,
}

fn expansion_at(rest: &str) -> bool {
    rest.chars()
        .next()
        .is_some_and(|c| c == '(' || c == '{' || c == '_' || c.is_alphanumeric())
}

fn finish(word: String, expands: bool, quoted: bool, tail: &str) -> Option<(Word, &str)> {
    if !quoted && word.is_empty() {
        return None;
    }
    Some((
        Word {
            text: word,
            expands,
        },
        tail,
    ))
}

fn read_word(text: &str) -> Option<(Word, &str)> {
    let text = text.trim_start_matches([' ', '\t']);
    let mut word = String::new();
    let (mut quoted, mut expands) = (false, false);
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | ';' | '|' | '&' | ')' | '<' | '>' => {
                return finish(word, expands, quoted, &text[at..]);
            }
            '\'' => {
                let rest = &text[at + 1..];
                let close = rest.find('\'')?;
                word.push_str(&rest[..close]);
                quoted = true;
                for _ in 0..rest[..close].chars().count() + 1 {
                    chars.next();
                }
            }
            '"' => {
                quoted = true;
                let mut closed = false;
                while let Some((i, d)) = chars.next() {
                    match d {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => match chars.next() {
                            Some((_, e)) if matches!(e, '"' | '\\' | '$' | '`') => word.push(e),
                            Some((_, e)) => {
                                word.push('\\');
                                word.push(e);
                            }
                            None => {}
                        },
                        '`' => {
                            expands = true;
                            word.push(d);
                        }
                        '$' => {
                            expands |= expansion_at(&text[i + 1..]);
                            word.push(d);
                        }
                        _ => word.push(d),
                    }
                }
                if !closed {
                    return None;
                }
            }
            '\\' => {
                if let Some((_, next)) = chars.next() {
                    word.push(next);
                }
            }
            '`' => {
                expands = true;
                word.push(c);
            }
            '$' => {
                expands |= expansion_at(&text[at + 1..]);
                word.push(c);
            }
            _ => word.push(c),
        }
    }
    finish(word, expands, quoted, "")
}

struct Call {
    at: usize,
    pattern: String,
}

fn pgrep_calls(command: &str) -> Vec<Call> {
    let mut found = Vec::new();
    for (at, _) in command.match_indices("pgrep") {
        if !word_at(command, at, "pgrep") {
            continue;
        }
        let mut rest = &command[at + "pgrep".len()..];
        let (mut full, mut exact, mut skip_value, mut scoped) = (false, false, false, false);
        let mut pattern = None;
        while let Some((word, tail)) = read_word(rest) {
            if tail.len() >= rest.len() {
                break;
            }
            rest = tail;
            if skip_value {
                skip_value = false;
                continue;
            }
            let text = word.text.as_str();
            if text == "--full" {
                full = true;
            } else if text == "--exact" {
                exact = true;
            } else if text.starts_with("--") {
                let name = text.split('=').next().unwrap_or(text);
                scoped |= SCOPING_LONG.contains(&name);
                skip_value = !text.contains('=') && VALUE_LONG.contains(&name);
            } else if let Some(flags) = text.strip_prefix('-').filter(|f| !f.is_empty()) {
                for (i, flag) in flags.char_indices() {
                    scoped |= SCOPING_FLAGS.contains(&flag);
                    match flag {
                        'f' => full = true,
                        'x' => exact = true,
                        c if VALUE_FLAGS.contains(&c) => {
                            skip_value = flags[i + c.len_utf8()..].is_empty();
                            break;
                        }
                        _ => {}
                    }
                }
            } else {
                pattern = Some(word);
                break;
            }
        }
        if let Some(word) = pattern.filter(|w| full && !exact && !scoped && !w.expands) {
            found.push(Call {
                at,
                pattern: word.text,
            });
        }
    }
    found
}

pub fn pgrep_patterns(command: &str) -> Vec<String> {
    pgrep_calls(command)
        .into_iter()
        .map(|call| call.pattern)
        .collect()
}

pub fn matches_itself(command: &str) -> bool {
    pgrep_calls(command).iter().any(|call| {
        waits_while_present(command, call.at)
            && Regex::new(&call.pattern).is_ok_and(|re| re.is_match(command))
    })
}

#[cfg(test)]
#[path = "shells_stuck_tests.rs"]
mod tests;
