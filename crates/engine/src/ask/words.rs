fn finish(word: &mut String, open: &mut bool, current: &mut Vec<String>) {
    if *open || !word.is_empty() {
        current.push(std::mem::take(word));
        *open = false;
    }
}

fn flush(current: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    if !current.is_empty() {
        out.push(std::mem::take(current));
    }
}

pub fn simple_commands(command: &str) -> Option<Vec<Vec<String>>> {
    let mut out = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut open = false;
    let mut chars = command.chars().peekable();
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
                        '\\' => match chars.next()? {
                            '\n' => {}
                            c @ ('"' | '\\' | '$' | '`') => word.push(c),
                            c => {
                                word.push('\\');
                                word.push(c);
                            }
                        },
                        '`' | '$' => return None,
                        '<' if chars.peek() == Some(&'<') => return None,
                        c => word.push(c),
                    }
                }
            }
            '\\' => match chars.next()? {
                '\n' => {}
                c => {
                    open = true;
                    word.push(c);
                }
            },
            '`' | '$' => return None,
            '<' if chars.peek() == Some(&'<') => return None,
            ';' | '\n' => {
                finish(&mut word, &mut open, &mut current);
                flush(&mut current, &mut out);
            }
            '&' | '|' => {
                if chars.peek() == Some(&c) {
                    chars.next();
                    finish(&mut word, &mut open, &mut current);
                    flush(&mut current, &mut out);
                } else if c == '|' {
                    finish(&mut word, &mut open, &mut current);
                    flush(&mut current, &mut out);
                } else {
                    open = true;
                    word.push(c);
                }
            }
            c if c.is_whitespace() => finish(&mut word, &mut open, &mut current),
            c => {
                open = true;
                word.push(c);
            }
        }
    }
    finish(&mut word, &mut open, &mut current);
    flush(&mut current, &mut out);
    Some(out)
}

pub(super) fn is_notchling(program: &str) -> bool {
    program
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("notchling"))
}

pub fn same_command(words: &[String], argv: &[String]) -> bool {
    match (words.split_first(), argv.split_first()) {
        (Some((a, rest_a)), Some((b, rest_b))) => {
            is_notchling(a) && is_notchling(b) && rest_a == rest_b
        }
        _ => false,
    }
}
