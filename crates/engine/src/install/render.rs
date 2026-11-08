//! The few JSON values we write into settings.json, rendered the way Claude Code and
//! `JSON.stringify(value, null, 2)` print them: `"key": value`, one member per line, slashes and
//! non-ASCII left as they are.

use serde_json::Value;

/// A value to insert. Members keep the order they are given.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Str(String),
    Num(u64),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

pub fn string(text: &str) -> String {
    // `to_string` on a `&str` cannot fail; the fallback keeps the output valid anyway.
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

impl Json {
    pub fn obj(members: Vec<(&str, Json)>) -> Json {
        Json::Obj(
            members
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    /// One line: `{"a": 1, "b": [2]}`.
    pub fn compact(&self) -> String {
        match self {
            Json::Str(s) => string(s),
            Json::Num(n) => n.to_string(),
            Json::Arr(items) => {
                let inner: Vec<String> = items.iter().map(Json::compact).collect();
                format!("[{}]", inner.join(", "))
            }
            Json::Obj(members) => {
                let inner: Vec<String> = members
                    .iter()
                    .map(|(k, v)| format!("{}: {}", string(k), v.compact()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
        }
    }

    /// Many lines. The first line starts where the caller puts it; every other line starts with
    /// `indent`, and each level inside adds `unit`. `nl` ends a line.
    pub fn pretty(&self, indent: &str, unit: &str, nl: &str) -> String {
        match self {
            Json::Arr(items) if !items.is_empty() => {
                let inner = format!("{indent}{unit}");
                let lines: Vec<String> = items
                    .iter()
                    .map(|i| format!("{inner}{}", i.pretty(&inner, unit, nl)))
                    .collect();
                format!("[{nl}{}{nl}{indent}]", lines.join(&format!(",{nl}")))
            }
            Json::Obj(members) if !members.is_empty() => {
                let inner = format!("{indent}{unit}");
                let lines: Vec<String> = members
                    .iter()
                    .map(|(k, v)| format!("{inner}{}: {}", string(k), v.pretty(&inner, unit, nl)))
                    .collect();
                format!("{{{nl}{}{nl}{indent}}}", lines.join(&format!(",{nl}")))
            }
            other => other.compact(),
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Json::Str(s) => Value::String(s.clone()),
            Json::Num(n) => Value::from(*n),
            Json::Arr(items) => Value::Array(items.iter().map(Json::to_value).collect()),
            Json::Obj(members) => Value::Object(
                members
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_value()))
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Json {
        Json::obj(vec![
            ("matcher", Json::Str("Artifact".into())),
            (
                "hooks",
                Json::Arr(vec![Json::obj(vec![
                    ("type", Json::Str("command".into())),
                    ("command", Json::Str("'/a b/notchling' hook".into())),
                    ("timeout", Json::Num(60)),
                ])]),
            ),
        ])
    }

    #[test]
    fn pretty_matches_json_stringify_with_two_spaces() {
        let text = sample().pretty("    ", "  ", "\n");
        assert_eq!(
            text,
            "{\n      \"matcher\": \"Artifact\",\n      \"hooks\": [\n        {\n          \"type\": \"command\",\n          \"command\": \"'/a b/notchling' hook\",\n          \"timeout\": 60\n        }\n      ]\n    }"
        );
    }

    #[test]
    fn compact_is_one_line_and_both_parse_to_the_same_value() {
        let one = sample().compact();
        assert!(!one.contains('\n'));
        assert_eq!(
            serde_json::from_str::<Value>(&one).unwrap(),
            sample().to_value()
        );
        let many = sample().pretty("", "\t", "\r\n");
        assert_eq!(
            serde_json::from_str::<Value>(&many).unwrap(),
            sample().to_value()
        );
    }

    #[test]
    fn slashes_and_unicode_are_not_escaped_but_quotes_and_controls_are() {
        assert_eq!(string("a/b é \"q\"\n"), "\"a/b é \\\"q\\\"\\n\"");
        assert_eq!(Json::Arr(vec![]).pretty("", "  ", "\n"), "[]");
        assert_eq!(Json::Obj(vec![]).compact(), "{}");
    }
}
