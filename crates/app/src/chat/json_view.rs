use serde_json::Value;

pub enum Shown {
    Pretty(String),
    Invalid { line: usize, column: usize },
}

pub fn read(text: &str) -> Shown {
    match serde_json::from_str::<Value>(text) {
        Ok(_) => Shown::Pretty(indent(text)),
        Err(e) => Shown::Invalid {
            line: e.line(),
            column: e.column(),
        },
    }
}

pub fn pretty(text: &str) -> Option<String> {
    match read(text) {
        Shown::Pretty(p) => Some(p),
        Shown::Invalid { .. } => None,
    }
}

fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn indent(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    let (mut depth, mut in_string, mut escaped, mut opened) = (0usize, false, false, None);
    for c in text.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c.is_whitespace() {
            continue;
        }
        if let Some(open) = opened.take() {
            let close = if open == '{' { '}' } else { ']' };
            if c == close {
                out.push(c);
                depth -= 1;
                continue;
            }
            newline(&mut out, depth);
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '{' | '[' => {
                out.push(c);
                depth += 1;
                opened = Some(c);
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(c);
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            _ => out.push(c),
        }
    }
    out
}
