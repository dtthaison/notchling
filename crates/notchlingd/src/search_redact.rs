use crate::reads_ops::mask_email;
use std::ops::Range;

const HIDDEN: &str = "***";

const PREFIXES: [(&str, usize); 13] = [
    ("sk-ant-", 20),
    ("sk-", 20),
    ("github_pat_", 20),
    ("ghp_", 20),
    ("gho_", 20),
    ("ghs_", 20),
    ("ghu_", 20),
    ("ghr_", 20),
    ("xoxb-", 15),
    ("xoxp-", 15),
    ("xoxa-", 15),
    ("xoxs-", 15),
    ("AIza", 39),
];

const GENERIC_MIN: usize = 32;

struct Span {
    start: usize,
    end: usize,
    with: String,
}

fn is_run(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'/' | b'=' | b'-' | b'.')
}

fn is_local(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn is_domain(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')
}

fn is_part(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'=')
}

fn emails(text: &str, out: &mut Vec<Span>) {
    let bytes = text.as_bytes();
    for (at, _) in bytes.iter().enumerate().filter(|(_, b)| **b == b'@') {
        let mut start = at;
        while start > 0 && is_local(bytes[start - 1]) {
            start -= 1;
        }
        let mut end = at + 1;
        while end < bytes.len() && is_domain(bytes[end]) {
            end += 1;
        }
        while end > at + 1 && matches!(bytes[end - 1], b'.' | b'-') {
            end -= 1;
        }
        let domain = &text[at + 1..end];
        let tld = domain.rsplit_once('.').map_or("", |(_, t)| t);
        let labels_ok = domain.split('.').all(|l| !l.is_empty());
        if start < at && labels_ok && tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic())
        {
            out.push(Span {
                start,
                end,
                with: mask_email(&text[start..end]),
            });
        }
    }
}

fn known(segment: &str) -> Option<String> {
    for (prefix, least) in PREFIXES {
        if segment.starts_with(prefix) && segment.len() >= least {
            return Some(format!("{prefix}{HIDDEN}"));
        }
    }
    if segment.starts_with("AKIA")
        && segment.len() >= 20
        && segment.as_bytes()[4..20]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    {
        return Some(format!("AKIA{HIDDEN}"));
    }
    None
}

fn mixed(segment: &str) -> bool {
    segment.len() >= GENERIC_MIN
        && segment.bytes().any(|b| b.is_ascii_alphabetic())
        && segment.bytes().any(|b| b.is_ascii_digit())
}

fn is_jwt(run: &str) -> bool {
    let parts: Vec<&str> = run.split('.').collect();
    parts.len() == 3
        && parts[0].starts_with("eyJ")
        && parts[0].len() >= 10
        && parts[1].len() >= 10
        && parts.iter().all(|p| p.bytes().all(is_part))
}

fn tokens(text: &str, out: &mut Vec<Span>) {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if !is_run(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_run(bytes[at]) {
            at += 1;
        }
        let mut end = at;
        while end > start && bytes[end - 1] == b'.' {
            end -= 1;
        }
        let run = &text[start..end];
        if is_jwt(run) {
            out.push(Span {
                start,
                end,
                with: format!("eyJ{HIDDEN}"),
            });
            continue;
        }
        let mut from = start;
        for segment in run.split('.') {
            let to = from + segment.len();
            if let Some(with) = known(segment) {
                out.push(Span {
                    start: from,
                    end: to,
                    with,
                });
            } else if mixed(segment) {
                out.push(Span {
                    start: from,
                    end: to,
                    with: HIDDEN.into(),
                });
            }
            from = to + 1;
        }
    }
}

fn spans(text: &str) -> Vec<Span> {
    let mut all = Vec::new();
    emails(text, &mut all);
    tokens(text, &mut all);
    all.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut merged: Vec<Span> = Vec::new();
    for span in all {
        match merged.last_mut() {
            Some(last) if span.start < last.end => {
                if span.end > last.end {
                    last.end = span.end;
                    last.with = HIDDEN.into();
                }
            }
            _ => merged.push(span),
        }
    }
    merged
}

pub fn mask(text: &str) -> String {
    mask_marked(text, &[]).0
}

pub fn mask_marked(text: &str, marks: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    let found = spans(text);
    if found.is_empty() {
        return (text.to_owned(), marks.to_vec());
    }
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for span in &found {
        out.push_str(&text[from..span.start]);
        out.push_str(&span.with);
        from = span.end;
    }
    out.push_str(&text[from..]);
    let shifted = marks
        .iter()
        .filter(|m| {
            !found
                .iter()
                .any(|s| m.start < s.end && s.start < m.end.max(m.start + 1))
        })
        .map(|m| {
            let moved = |at: usize| -> usize {
                let before: isize = found
                    .iter()
                    .filter(|s| s.end <= at)
                    .map(|s| s.with.len() as isize - (s.end - s.start) as isize)
                    .sum();
                (at as isize + before) as usize
            };
            moved(m.start)..moved(m.end)
        })
        .collect();
    (out, shifted)
}

#[cfg(test)]
#[path = "search_redact_tests.rs"]
mod tests;
