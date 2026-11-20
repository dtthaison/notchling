fn is_substitution(c: char, next: Option<&char>) -> bool {
    c == '`' || (c == '$' && next == Some(&'('))
}

pub fn words(command: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut open = false;
    let mut chars = command.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                open = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => word.push(c),
                    }
                }
            }
            '"' => {
                open = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' if matches!(chars.peek(), Some('"' | '\\' | '$' | '`')) => {
                            word.push(chars.next()?)
                        }
                        c if is_substitution(c, chars.peek()) => return None,
                        c => word.push(c),
                    }
                }
            }
            '\\' => {
                open = true;
                word.push(chars.next()?);
            }
            '\n' | '\r' | ';' | '&' | '|' | '<' | '>' | '(' | ')' | '{' | '}' => return None,
            c if is_substitution(c, chars.peek()) => return None,
            c if c.is_whitespace() => {
                if open || !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                    open = false;
                }
            }
            c => {
                open = true;
                word.push(c);
            }
        }
    }
    if open || !word.is_empty() {
        out.push(word);
    }
    (!out.is_empty()).then_some(out)
}

pub fn program(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

pub fn is_flag(word: &str) -> bool {
    word.len() > 1 && word.starts_with('-')
}

pub fn has_glob_or_var(word: &str) -> bool {
    word.contains(['*', '?', '[', ']', '$', '~', '=', '!'])
}

pub fn seconds(word: &str) -> Option<f64> {
    let split = word
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(word.len());
    let (number, unit) = word.split_at(split);
    let value: f64 = number.parse().ok()?;
    let scale = match unit {
        "" | "s" => 1.0,
        "m" => 60.0,
        "h" => 3_600.0,
        "d" => 86_400.0,
        _ => return None,
    };
    Some(value * scale)
}

pub fn loops_on_sleep(command: &str) -> bool {
    let trimmed = command.trim_start();
    let first = trimmed
        .split(|c: char| c.is_whitespace() || c == ';')
        .next()
        .unwrap_or("");
    if first != "while" && first != "until" {
        return false;
    }
    trimmed
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '`'))
        .any(|w| w == "sleep")
}
