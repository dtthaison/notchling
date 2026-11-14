#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TomlEditError {
    Parse(&'static str),
    Duplicate,
    NotABool,
}

impl std::fmt::Display for TomlEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TomlEditError::Parse(why) => write!(f, "{why}"),
            TomlEditError::Duplicate => write!(f, "duplicate_key"),
            TomlEditError::NotABool => write!(f, "not_a_bool"),
        }
    }
}

type Failure = TomlEditError;

struct Stmt {
    table: Option<String>,
    array_table: bool,
    key: String,
    start: usize,
    end: usize,
    value: (usize, usize),
}

struct Header {
    name: String,
    array: bool,
    start: usize,
    end: usize,
}

struct Scan {
    stmts: Vec<Stmt>,
    headers: Vec<Header>,
    leading_end: usize,
}

fn bad(why: &'static str) -> Failure {
    TomlEditError::Parse(why)
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    i
}

fn line_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i] != b'\n' {
        i += 1;
    }
    if i < b.len() {
        i + 1
    } else {
        i
    }
}

fn quoted(b: &[u8], i: usize) -> Result<usize, Failure> {
    let q = b[i];
    if b[i..].starts_with(&[q, q, q]) {
        let mut j = i + 3;
        loop {
            if j >= b.len() {
                return Err(bad("unterminated"));
            }
            if q == b'"' && b[j] == b'\\' {
                j += 2;
                continue;
            }
            if b[j..].starts_with(&[q, q, q]) {
                let mut k = j + 3;
                let mut extra = 0;
                while k < b.len() && b[k] == q && extra < 2 {
                    k += 1;
                    extra += 1;
                }
                return Ok(k);
            }
            j += 1;
        }
    }
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\n' => return Err(bad("unterminated")),
            b'\\' if q == b'"' => j += 2,
            c if c == q => return Ok(j + 1),
            _ => j += 1,
        }
    }
    Err(bad("unterminated"))
}

fn nested(b: &[u8], i: usize) -> Result<usize, Failure> {
    let mut depth = 0usize;
    let mut j = i;
    while j < b.len() {
        match b[j] {
            b'"' | b'\'' => {
                j = quoted(b, j)?;
                continue;
            }
            b'#' => {
                j = line_end(b, j);
                continue;
            }
            b'[' | b'{' => depth += 1,
            b']' | b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok(j + 1);
                }
            }
            _ => {}
        }
        j += 1;
    }
    Err(bad("unterminated"))
}

fn value_end(b: &[u8], i: usize) -> Result<usize, Failure> {
    match b.get(i) {
        None | Some(b'\n') | Some(b'\r') | Some(b'#') => Err(bad("bad_value")),
        Some(b'"') | Some(b'\'') => quoted(b, i),
        Some(b'[') | Some(b'{') => nested(b, i),
        Some(_) => {
            let mut j = i;
            while j < b.len()
                && !matches!(
                    b[j],
                    b' ' | b'\t' | b'\n' | b'\r' | b'#' | b',' | b']' | b'}'
                )
            {
                j += 1;
            }
            Ok(j)
        }
    }
}

fn is_bare(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

fn key_part(text: &str, j: usize) -> Result<(usize, String), Failure> {
    let b = text.as_bytes();
    match b.get(j) {
        Some(b'"') | Some(b'\'') => {
            if b[j..].starts_with(&[b[j], b[j], b[j]]) {
                return Err(bad("bad_key"));
            }
            let end = quoted(b, j)?;
            Ok((end, text[j + 1..end - 1].to_string()))
        }
        _ => {
            let mut k = j;
            while k < b.len() && is_bare(b[k]) {
                k += 1;
            }
            if k == j {
                return Err(bad("bad_key"));
            }
            Ok((k, text[j..k].to_string()))
        }
    }
}

fn key_end(text: &str, i: usize) -> Result<(usize, String), Failure> {
    let b = text.as_bytes();
    let mut j = i;
    let mut parts: Vec<String> = Vec::new();
    loop {
        j = skip_ws(b, j);
        let (end, part) = key_part(text, j)?;
        parts.push(part);
        let k = skip_ws(b, end);
        if b.get(k) == Some(&b'.') {
            j = k + 1;
            continue;
        }
        return Ok((k, parts.join(".")));
    }
}

fn rest_of_line(b: &[u8], i: usize, why: &'static str) -> Result<usize, Failure> {
    let j = skip_ws(b, i);
    match b.get(j) {
        None => Ok(j),
        Some(b'\n') => Ok(j + 1),
        Some(b'\r') if b.get(j + 1) == Some(&b'\n') => Ok(j + 2),
        Some(b'#') => Ok(line_end(b, j)),
        Some(_) => Err(bad(why)),
    }
}

fn header_at(text: &str, i: usize) -> Result<Header, Failure> {
    let b = text.as_bytes();
    let array = b.get(i + 1) == Some(&b'[');
    let open = if array { 2 } else { 1 };
    let mut j = i + open;
    let name_start = j;
    loop {
        match b.get(j) {
            None | Some(b'\n') => return Err(bad("bad_header")),
            Some(b'"') | Some(b'\'') => j = quoted(b, j)?,
            Some(b']') => break,
            Some(_) => j += 1,
        }
    }
    let raw = &text[name_start..j];
    j += 1;
    if array {
        if b.get(j) != Some(&b']') {
            return Err(bad("bad_header"));
        }
        j += 1;
    }
    let end = rest_of_line(b, j, "bad_header")?;
    let name = raw
        .split('.')
        .map(|p| p.trim().trim_matches(|c| c == '"' || c == '\''))
        .collect::<Vec<_>>()
        .join(".");
    if name.is_empty() {
        return Err(bad("bad_header"));
    }
    Ok(Header {
        name,
        array,
        start: i,
        end,
    })
}

fn scan(text: &str) -> Result<Scan, Failure> {
    let b = text.as_bytes();
    let mut out = Scan {
        stmts: Vec::new(),
        headers: Vec::new(),
        leading_end: 0,
    };
    let mut table: Option<String> = None;
    let mut in_array = false;
    let mut seen = false;
    let mut pos = 0;
    while pos < b.len() {
        let line_start = pos;
        let i = skip_ws(b, pos);
        let blank = matches!(b.get(i), None | Some(b'\n') | Some(b'\r') | Some(b'#'));
        if blank {
            pos = line_end(b, i);
            if !seen {
                out.leading_end = pos;
            }
            continue;
        }
        seen = true;
        if b[i] == b'[' {
            let header = header_at(text, line_start)?;
            if !header.array
                && out
                    .headers
                    .iter()
                    .any(|h| !h.array && h.name == header.name)
            {
                return Err(TomlEditError::Duplicate);
            }
            table = Some(header.name.clone());
            in_array = header.array;
            pos = header.end;
            out.headers.push(header);
            continue;
        }
        let (after_key, key) = key_end(text, i)?;
        if b.get(after_key) != Some(&b'=') {
            return Err(bad("no_equals"));
        }
        let value_start = skip_ws(b, after_key + 1);
        let value_stop = value_end(b, value_start)?;
        let end = rest_of_line(b, value_stop, "bad_value")?;
        out.stmts.push(Stmt {
            table: table.clone(),
            array_table: in_array,
            key,
            start: line_start,
            end,
            value: (value_start, value_stop),
        });
        pos = end;
    }
    if !seen {
        out.leading_end = b.len();
    }
    Ok(out)
}

fn check_name(name: &str) -> Result<(), Failure> {
    if !name.is_empty() && name.bytes().all(is_bare) {
        Ok(())
    } else {
        Err(bad("bad_key"))
    }
}

fn matching<'a>(scan: &'a Scan, table: Option<&str>, key: &str) -> Vec<&'a Stmt> {
    let dotted = table.map(|t| format!("{t}.{key}"));
    scan.stmts
        .iter()
        .filter(|s| {
            !s.array_table
                && ((s.table.as_deref() == table && s.key == key)
                    || (table.is_some() && s.table.is_none() && Some(&s.key) == dotted.as_ref()))
        })
        .collect()
}

fn bool_of(text: &str, stmt: &Stmt) -> Result<bool, Failure> {
    match &text[stmt.value.0..stmt.value.1] {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(TomlEditError::NotABool),
    }
}

pub fn get_bool(text: &str, table: Option<&str>, key: &str) -> Result<Option<bool>, TomlEditError> {
    check_name(key)?;
    if let Some(t) = table {
        check_name(t)?;
    }
    let scan = scan(text)?;
    let found = matching(&scan, table, key);
    match found.as_slice() {
        [] => Ok(None),
        [one] => bool_of(text, one).map(Some),
        _ => Err(TomlEditError::Duplicate),
    }
}

fn eol_of(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn insert_line(text: &str, at: usize, line: &str, extra_blank: bool) -> String {
    let eol = eol_of(text);
    let mut out = String::with_capacity(text.len() + line.len() + 4);
    out.push_str(&text[..at]);
    if at > 0 && !text[..at].ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str(line);
    out.push_str(eol);
    if extra_blank {
        out.push_str(eol);
    }
    out.push_str(&text[at..]);
    out
}

type Shape = (Vec<(Option<String>, String, String)>, Vec<String>);

fn shape(text: &str, table: Option<&str>, key: &str) -> Result<Shape, Failure> {
    let scan = scan(text)?;
    let others = scan
        .stmts
        .iter()
        .filter(|s| {
            !matching(&scan, table, key)
                .iter()
                .any(|m| std::ptr::eq(*m, *s))
        })
        .map(|s| {
            (
                s.table.clone(),
                s.key.clone(),
                text[s.value.0..s.value.1].to_string(),
            )
        })
        .collect();
    let headers = scan.headers.iter().map(|h| h.name.clone()).collect();
    Ok((others, headers))
}

fn verify(
    before: &str,
    after: &str,
    table: Option<&str>,
    key: &str,
    want: Option<bool>,
) -> Result<(), Failure> {
    if get_bool(after, table, key)? != want {
        return Err(bad("check_failed"));
    }
    let (old_others, old_headers) = shape(before, table, key)?;
    let (new_others, new_headers) = shape(after, table, key)?;
    let created = table.is_some_and(|t| !old_headers.iter().any(|h| h == t));
    let headers_same = new_headers == old_headers
        || (created && new_headers.len() == old_headers.len() + 1 && {
            let mut without = new_headers.clone();
            without.retain(|h| Some(h.as_str()) != table);
            without == old_headers
        });
    if old_others != new_others || !headers_same {
        return Err(bad("check_failed"));
    }
    Ok(())
}

pub fn set_bool(
    text: &str,
    table: Option<&str>,
    key: &str,
    value: bool,
) -> Result<String, TomlEditError> {
    check_name(key)?;
    if let Some(t) = table {
        check_name(t)?;
    }
    let scan = scan(text)?;
    let found = matching(&scan, table, key);
    let word = if value { "true" } else { "false" };
    let out = match found.as_slice() {
        [one] => {
            bool_of(text, one)?;
            if &text[one.value.0..one.value.1] == word {
                return Ok(text.to_string());
            }
            let mut out = text.to_string();
            out.replace_range(one.value.0..one.value.1, word);
            out
        }
        [] => insert_new(text, &scan, table, key, word),
        _ => return Err(TomlEditError::Duplicate),
    };
    verify(text, &out, table, key, Some(value))?;
    Ok(out)
}

fn insert_new(text: &str, scan: &Scan, table: Option<&str>, key: &str, word: &str) -> String {
    let line = format!("{key} = {word}");
    let Some(name) = table else {
        let top: Vec<&Stmt> = scan.stmts.iter().filter(|s| s.table.is_none()).collect();
        if let Some(last) = top.last() {
            return insert_line(text, last.end, &line, false);
        }
        let first_header = scan.headers.first().map(|h| h.start);
        let at = scan.leading_end;
        let before_header = first_header == Some(at);
        return insert_line(text, at, &line, before_header);
    };
    let header = scan.headers.iter().find(|h| !h.array && h.name == name);
    match header {
        Some(h) => {
            let last = scan
                .stmts
                .iter()
                .filter(|s| !s.array_table && s.table.as_deref() == Some(name))
                .map(|s| s.end)
                .max();
            insert_line(text, last.unwrap_or(h.end), &line, false)
        }
        None => {
            let eol = eol_of(text);
            let mut out = text.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push_str(eol);
            }
            let ends_blank = out.is_empty() || out.ends_with("\n\n") || out.ends_with("\r\n\r\n");
            if !ends_blank {
                out.push_str(eol);
            }
            out.push_str(&format!("[{name}]{eol}{line}{eol}"));
            out
        }
    }
}

pub fn remove(text: &str, table: Option<&str>, key: &str) -> Result<String, TomlEditError> {
    check_name(key)?;
    if let Some(t) = table {
        check_name(t)?;
    }
    let scan = scan(text)?;
    let found = matching(&scan, table, key);
    match found.as_slice() {
        [] => Ok(text.to_string()),
        [one] => {
            let mut out = text.to_string();
            out.replace_range(one.start..one.end, "");
            verify(text, &out, table, key, None)?;
            Ok(out)
        }
        _ => Err(TomlEditError::Duplicate),
    }
}
