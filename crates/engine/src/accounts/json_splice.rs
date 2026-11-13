use serde_json::{Map, Value};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpliceError {
    NotAnObject,
    Duplicate,
    Mismatch,
}

impl fmt::Display for SpliceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SpliceError::NotAnObject => "the file isn't a JSON object",
            SpliceError::Duplicate => "the key is in the file more than once",
            SpliceError::Mismatch => "the change didn't read back as written",
        })
    }
}

pub fn set_top_level(text: &str, key: &str, value: &Value) -> Result<String, SpliceError> {
    let before = match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => map,
        _ => return Err(SpliceError::NotAnObject),
    };
    let spans = top_level_spans(text.as_bytes()).ok_or(SpliceError::NotAnObject)?;
    let found: Vec<&Span> = spans.members.iter().filter(|s| s.key == key).collect();
    let indent = spans.indent.clone();
    let rendered = render(value, indent.as_deref());
    let out = match found.as_slice() {
        [] => {
            let open = spans.open + 1;
            let colon = if indent.is_some() { ": " } else { ":" };
            let entry = format!("{}{colon}{rendered}", Value::String(key.into()));
            let sep = if spans.members.is_empty() { "" } else { "," };
            match &indent {
                Some(pad) if spans.members.is_empty() => {
                    format!(
                        "{}\n{pad}{entry}\n{}",
                        &text[..open],
                        text[open..].trim_start()
                    )
                }
                Some(pad) => format!("{}\n{pad}{entry}{sep}{}", &text[..open], &text[open..]),
                None => format!("{}{entry}{sep}{}", &text[..open], &text[open..]),
            }
        }
        [span] => format!("{}{rendered}{}", &text[..span.start], &text[span.end..]),
        _ => return Err(SpliceError::Duplicate),
    };
    check(&before, &out, key, value)?;
    Ok(out)
}

fn check(
    before: &Map<String, Value>,
    out: &str,
    key: &str,
    value: &Value,
) -> Result<(), SpliceError> {
    let Ok(Value::Object(after)) = serde_json::from_str::<Value>(out) else {
        return Err(SpliceError::Mismatch);
    };
    let mut expected = before.clone();
    expected.insert(key.into(), value.clone());
    if after == expected {
        Ok(())
    } else {
        Err(SpliceError::Mismatch)
    }
}

fn render(value: &Value, indent: Option<&str>) -> String {
    match indent {
        None => value.to_string(),
        Some(pad) => {
            let pretty = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
            pretty.replace('\n', &format!("\n{pad}"))
        }
    }
}

struct Span {
    key: String,
    start: usize,
    end: usize,
}

struct Spans {
    open: usize,
    members: Vec<Span>,
    indent: Option<String>,
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

fn skip_string(b: &[u8], mut i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'"') {
        return None;
    }
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

fn skip_value(b: &[u8], i: usize) -> Option<usize> {
    match b.get(i)? {
        b'"' => skip_string(b, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'"' => {
                        j = skip_string(b, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            None
        }
        _ => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')
            {
                j += 1;
            }
            (j > i).then_some(j)
        }
    }
}

fn top_level_spans(b: &[u8]) -> Option<Spans> {
    let open = skip_ws(b, 0);
    if b.get(open) != Some(&b'{') {
        return None;
    }
    let mut spans = Spans {
        open,
        members: Vec::new(),
        indent: None,
    };
    let mut i = skip_ws(b, open + 1);
    if b.get(i) == Some(&b'}') {
        return (skip_ws(b, i + 1) == b.len()).then_some(spans);
    }
    loop {
        let key_start = i;
        let key_end = skip_string(b, key_start)?;
        if spans.members.is_empty() {
            let gap = std::str::from_utf8(&b[open + 1..key_start]).ok()?;
            spans.indent = gap
                .rfind('\n')
                .map(|nl| gap[nl + 1..].to_string())
                .filter(|pad| !pad.is_empty());
        }
        let key: String = serde_json::from_slice(&b[key_start..key_end]).ok()?;
        i = skip_ws(b, key_end);
        if b.get(i) != Some(&b':') {
            return None;
        }
        let start = skip_ws(b, i + 1);
        let end = skip_value(b, start)?;
        spans.members.push(Span { key, start, end });
        i = skip_ws(b, end);
        match b.get(i)? {
            b',' => i = skip_ws(b, i + 1),
            b'}' => return (skip_ws(b, i + 1) == b.len()).then_some(spans),
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CLAUDE_JSON: &str = "{\n  \"numStartups\": 12,\n  \"oauthAccount\": {\n    \"accountUuid\": \"old\",\n    \"emailAddress\": \"old@example.com\"\n  },\n  \"projects\": {\n    \"/w\": {\"allowedTools\": [], \"cost\": 1.50}\n  },\n  \"userID\": \"x\"\n}\n";

    #[test]
    fn only_the_value_changes_and_every_other_byte_stays() {
        let out =
            set_top_level(CLAUDE_JSON, "oauthAccount", &json!({"accountUuid": "new"})).unwrap();
        assert_eq!(
            out,
            "{\n  \"numStartups\": 12,\n  \"oauthAccount\": {\n    \"accountUuid\": \"new\"\n  },\n  \"projects\": {\n    \"/w\": {\"allowedTools\": [], \"cost\": 1.50}\n  },\n  \"userID\": \"x\"\n}\n"
        );
        assert!(out.contains("\"cost\": 1.50"));
    }

    #[test]
    fn a_missing_key_is_added_after_the_brace() {
        let out = set_top_level("{\n  \"a\": 1\n}", "oauthAccount", &json!({"k": "v"})).unwrap();
        assert_eq!(
            out,
            "{\n  \"oauthAccount\": {\n    \"k\": \"v\"\n  },\n  \"a\": 1\n}"
        );
        let compact = set_top_level("{\"a\":1}", "claudeAiOauth", &json!({"t": 1})).unwrap();
        assert_eq!(compact, "{\"claudeAiOauth\":{\"t\":1},\"a\":1}");
        assert_eq!(set_top_level("{}", "k", &json!(1)).unwrap(), "{\"k\":1}");
    }

    #[test]
    fn a_compact_keychain_item_keeps_its_mcp_logins() {
        let item = r#"{"claudeAiOauth":{"accessToken":"test-old"},"mcpOAuth":{"srv|1":{"accessToken":"test-mcp"}}}"#;
        let out =
            set_top_level(item, "claudeAiOauth", &json!({"accessToken": "test-new"})).unwrap();
        assert_eq!(
            out,
            r#"{"claudeAiOauth":{"accessToken":"test-new"},"mcpOAuth":{"srv|1":{"accessToken":"test-mcp"}}}"#
        );
    }

    #[test]
    fn strings_with_braces_and_escapes_dont_confuse_it() {
        let text = r#"{"a": "} \" {", "oauthAccount": "x", "b": ["]", {"c": "}"}]}"#;
        let out = set_top_level(text, "oauthAccount", &json!("y")).unwrap();
        assert_eq!(
            out,
            r#"{"a": "} \" {", "oauthAccount": "y", "b": ["]", {"c": "}"}]}"#
        );
    }

    #[test]
    fn odd_texts_are_refused() {
        for bad in ["", "[]", "not json", "{\"a\":1} trailing", "\"text\""] {
            assert_eq!(
                set_top_level(bad, "k", &json!(1)),
                Err(SpliceError::NotAnObject),
                "{bad}"
            );
        }
        assert_eq!(
            set_top_level("{\"k\":1,\"k\":2}", "k", &json!(3)),
            Err(SpliceError::Duplicate)
        );
    }
}
