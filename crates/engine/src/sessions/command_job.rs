use std::io::{Read, Seek, SeekFrom};

pub fn readable(command: &str) -> String {
    let chars: Vec<char> = command.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i..].starts_with(&['e', 'n', 'v']) {
            if let Some(end) = env_prefix_end(&chars, i + 3, 0) {
                out.push_str("env … ");
                i = end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn env_prefix_end(c: &[char], at: usize, count: usize) -> Option<usize> {
    if let Some(end) = assignment_end(c, at) {
        if let Some(found) = env_prefix_end(c, end, count + 1) {
            return Some(found);
        }
        if c.get(end) == Some(&' ') {
            return Some(end + 1);
        }
    }
    if count > 0 && c.get(at) == Some(&' ') {
        return Some(at + 1);
    }
    None
}

fn assignment_end(c: &[char], at: usize) -> Option<usize> {
    if c.get(at) != Some(&' ') {
        return None;
    }
    let mut i = at + 1;
    let first = *c.get(i)?;
    if !(first.is_ascii_uppercase() || first == '_') {
        return None;
    }
    while c
        .get(i)
        .is_some_and(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || *ch == '_')
    {
        i += 1;
    }
    if c.get(i) != Some(&'=') {
        return None;
    }
    i += 1;
    for quote in ['\'', '"'] {
        if c.get(i) == Some(&quote) {
            if let Some(close) = c[i + 1..].iter().position(|ch| *ch == quote) {
                let end = i + 1 + close + 1;
                if c.get(end).is_none_or(|ch| ch.is_whitespace()) {
                    return Some(end);
                }
            }
        }
    }
    while c.get(i).is_some_and(|ch| !ch.is_whitespace()) {
        i += 1;
    }
    Some(i)
}

pub fn output_path(command: &str, cwd: &str, home: &str) -> Option<String> {
    let c: Vec<char> = command.chars().collect();
    let mut last: Option<String> = None;
    let mut pos = 0;
    while pos <= c.len() {
        let mut found = if pos == 0 { redirect_at(&c, 0) } else { None };
        if found.is_none() && c.get(pos).is_some_and(|ch| is_delim(*ch)) {
            found = redirect_at(&c, pos + 1);
        }
        match found {
            Some((target, end)) => {
                last = Some(target);
                pos = end;
            }
            None => pos += 1,
        }
    }
    let mut path = last?;
    if path == "/dev/null" {
        return None;
    }
    if let Some(rest) = path.strip_prefix("~/") {
        path = format!("{}/{rest}", home.trim_end_matches('/'));
    }
    Some(if path.starts_with('/') {
        path
    } else {
        format!("{}/{path}", cwd.trim_end_matches('/'))
    })
}

fn is_delim(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, ';' | '&' | '|')
}

fn redirect_at(c: &[char], at: usize) -> Option<(String, usize)> {
    let mut i = at;
    if c.get(i) == Some(&'1') {
        i += 1;
    }
    if c.get(i) != Some(&'>') {
        return None;
    }
    i += 1;
    if c.get(i) == Some(&'>') {
        i += 1;
    }
    while c.get(i).is_some_and(|ch| ch.is_whitespace()) {
        i += 1;
    }
    for quote in ['\'', '"'] {
        if c.get(i) == Some(&quote) {
            if let Some(close) = c[i + 1..].iter().position(|ch| *ch == quote) {
                if close > 0 {
                    let text: String = c[i + 1..i + 1 + close].iter().collect();
                    return Some((text, i + close + 2));
                }
            }
        }
    }
    let start = i;
    while c
        .get(i)
        .is_some_and(|ch| !ch.is_whitespace() && !matches!(ch, ';' | '&' | '|' | '<' | '>'))
    {
        i += 1;
    }
    (i > start).then(|| (c[start..i].iter().collect(), i))
}

pub fn tail(path: &str, max_lines: usize) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let size = file.seek(SeekFrom::End(0)).ok()?;
    let start = size.saturating_sub(65_536);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).ok()?;
    let text = String::from_utf8_lossy(&data);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let from = lines.len().saturating_sub(max_lines);
    Some(lines[from..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::super::testutil::TempDir;
    use super::*;

    #[test]
    fn output_path_finds_the_redirect() {
        let sd = "cd /p && env HOME=/h PATH=/a:/b /venv/python3 /x/sd.py autopilot > /repo/.git/run.log 2>&1";
        let at = |c: &str| output_path(c, "/p", "/home/me");
        assert_eq!(at(sd).as_deref(), Some("/repo/.git/run.log"));
        assert_eq!(
            at("make test >> build.log").as_deref(),
            Some("/p/build.log")
        );
        assert_eq!(
            at("run > 'my out.log' 2>&1").as_deref(),
            Some("/p/my out.log")
        );
        assert_eq!(at("run 2>&1"), None);
        assert_eq!(at("run > /dev/null"), None);
        assert_eq!(at("npm test"), None);
    }

    #[test]
    fn output_path_edge_cases() {
        let at = |c: &str| output_path(c, "/p", "/home/me");
        assert_eq!(at("run 1> out.log").as_deref(), Some("/p/out.log"));
        assert_eq!(at("run > ~/out.log").as_deref(), Some("/home/me/out.log"));
        assert_eq!(at("run > \"a b.log\"").as_deref(), Some("/p/a b.log"));
        assert_eq!(
            at("a > first.log; b > second.log").as_deref(),
            Some("/p/second.log")
        );
        assert_eq!(at(">out.log run").as_deref(), Some("/p/out.log"));
        assert_eq!(at("run >&2"), None);
        assert_eq!(at("run 2> err.log"), None);
    }

    #[test]
    fn readable_shortens_the_env_prefix() {
        assert_eq!(
            readable("cd /p && env HOME=/h PATH=/a:/b python3 sd.py"),
            "cd /p && env … python3 sd.py"
        );
        assert_eq!(readable("env A='x y' B=\"z w\" run"), "env … run");
        assert_eq!(readable("make test"), "make test");
        assert_eq!(readable("env run"), "env run");
    }

    #[test]
    fn tail_keeps_the_last_lines() {
        let tmp = TempDir::new("tail");
        let text: String = (1..=300).map(|n| format!("line {n}\n")).collect();
        let p = tmp.write("x.log", &text);
        assert_eq!(
            tail(&p.display().to_string(), 3).as_deref(),
            Some("line 298\nline 299\nline 300")
        );
        assert_eq!(tail("/nonexistent/file.log", 3), None);
    }

    #[test]
    fn tail_drops_a_partial_first_line_of_a_big_file() {
        let tmp = TempDir::new("tail-big");
        let text: String = (1..=20000).map(|n| format!("line number {n}\n")).collect();
        let p = tmp.write("big.log", &text);
        let out = tail(&p.display().to_string(), 100_000).unwrap_or_default();
        assert!(out.starts_with("line number "), "{}", &out[..20]);
        assert!(out.ends_with("line number 20000"));
        assert!(out.len() <= 65_536);
    }
}
