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
            '#' if !open && word.is_empty() => {
                if chars.any(|c| c == '\n') {
                    return None;
                }
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

pub fn segments(command: &str) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut open = false;
    let mut chars = command.chars().peekable();
    let flush = |words: &mut Vec<String>, word: &mut String, open: &mut bool| {
        if *open || !word.is_empty() {
            words.push(std::mem::take(word));
            *open = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                open = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    word.push(c);
                }
            }
            '"' => {
                open = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' if matches!(chars.peek(), Some('"' | '\\' | '$' | '`')) => {
                            word.extend(chars.next())
                        }
                        c => word.push(c),
                    }
                }
            }
            '\\' => {
                open = true;
                word.extend(chars.next());
            }
            '#' if !open && word.is_empty() => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                flush(&mut words, &mut word, &mut open);
                out.push(std::mem::take(&mut words));
            }
            '\n' | '\r' | ';' | '&' | '|' | '(' | ')' | '{' | '}' | '`' => {
                flush(&mut words, &mut word, &mut open);
                out.push(std::mem::take(&mut words));
            }
            c if c.is_whitespace() => flush(&mut words, &mut word, &mut open),
            c => {
                open = true;
                word.push(c);
            }
        }
    }
    flush(&mut words, &mut word, &mut open);
    out.push(words);
    out.retain(|w| !w.is_empty());
    out
}

fn command_word(words: &[String]) -> Option<&str> {
    words
        .iter()
        .map(String::as_str)
        .find(|w| !matches!(*w, "do" | "then" | "else" | "time" | "!"))
}

pub fn loops_on_sleep(command: &str) -> bool {
    let parts = segments(command);
    let looping = parts.iter().any(|w| match w[0].as_str() {
        "while" | "until" => true,
        "for" => w.len() == 1,
        _ => false,
    });
    looping
        && parts
            .iter()
            .any(|w| command_word(w).is_some_and(|c| program(c) == "sleep"))
}
