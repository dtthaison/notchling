use regex::Regex;

pub const STUCK_AFTER_S: u64 = 30 * 60;

const VALUE_FLAGS: [char; 10] = ['F', 'G', 'P', 'U', 'd', 'g', 's', 't', 'u', 'O'];

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

pub fn looks_stuck(command: &str, running_s: Option<u64>) -> Option<Stuck> {
    if matches_itself(command) {
        return Some(Stuck::Itself);
    }
    (is_wait_loop(command) && running_s.is_some_and(|s| s > STUCK_AFTER_S))
        .then_some(Stuck::LongLoop)
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

pub fn is_wait_loop(command: &str) -> bool {
    (has_word(command, "while") || has_word(command, "until"))
        && has_word(command, "pgrep")
        && has_word(command, "sleep")
}

fn read_word(text: &str) -> Option<(String, &str)> {
    let text = text.trim_start_matches([' ', '\t']);
    let mut word = String::new();
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | ';' | '|' | '&' | ')' | '<' | '>' => {
                return (!word.is_empty()).then(|| (word, &text[at..]));
            }
            '\'' | '"' => {
                let rest = &text[at + 1..];
                let Some(close) = rest.find(c) else {
                    return (!word.is_empty()).then(|| (word, &text[at..]));
                };
                let inner = &rest[..close];
                if c == '"' {
                    word.push_str(&inner.replace("\\\"", "\"").replace("\\\\", "\\"));
                } else {
                    word.push_str(inner);
                }
                for _ in 0..inner.chars().count() + 1 {
                    chars.next();
                }
            }
            '\\' => {
                if let Some((_, next)) = chars.next() {
                    word.push(next);
                }
            }
            _ => word.push(c),
        }
    }
    (!word.is_empty()).then(|| (word, ""))
}

pub fn pgrep_patterns(command: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at, _) in command.match_indices("pgrep") {
        if !word_at(command, at, "pgrep") {
            continue;
        }
        let mut rest = &command[at + "pgrep".len()..];
        let (mut full, mut exact, mut skip_value) = (false, false, false);
        let mut pattern = None;
        while let Some((word, tail)) = read_word(rest) {
            rest = tail;
            if skip_value {
                skip_value = false;
                continue;
            }
            if word == "--full" {
                full = true;
            } else if word == "--exact" {
                exact = true;
            } else if word.starts_with("--") {
            } else if let Some(flags) = word.strip_prefix('-').filter(|f| !f.is_empty()) {
                for (i, flag) in flags.char_indices() {
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
        if let Some(pattern) = pattern.filter(|_| full && !exact) {
            found.push(pattern);
        }
    }
    found
}

pub fn matches_itself(command: &str) -> bool {
    pgrep_patterns(command)
        .iter()
        .any(|pattern| match Regex::new(pattern) {
            Ok(re) => re.is_match(command),
            Err(_) => command.contains(pattern.as_str()),
        })
}

#[cfg(test)]
#[path = "shells_stuck_tests.rs"]
mod tests;
