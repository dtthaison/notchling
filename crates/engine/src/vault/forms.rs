use super::redact::{wipe_string, MIN_HIDDEN};
use std::collections::BTreeMap;

const HTML_UNSAFE: [char; 5] = ['<', '>', '&', '\u{2028}', '\u{2029}'];

fn json_inner(text: &str) -> String {
    let quoted = serde_json::to_string(text).unwrap_or_default();
    let inner = quoted
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_default()
        .to_string();
    wipe_string(quoted);
    inner
}

fn json_go(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\u{8}' => out.push_str("\\u0008"),
            '\u{c}' => out.push_str("\\u000c"),
            other => {
                let mut buf = [0u8; 4];
                let piece = json_inner(other.encode_utf8(&mut buf));
                out.push_str(&piece);
                wipe_string(piece);
            }
        }
    }
    out
}

fn ascii_escaped(text: &str, upper: bool) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_ascii() {
            out.push(ch);
        } else {
            let mut units = [0u16; 2];
            for unit in ch.encode_utf16(&mut units) {
                if upper {
                    out.push_str(&format!("\\u{unit:04X}"));
                } else {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out
}

fn html_escaped(json: &str) -> String {
    json.replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn forms_of(text: &str, into: &mut Vec<String>) {
    into.push(text.to_string());
    let mut bases = vec![json_inner(text)];
    if text.contains(['\u{8}', '\u{c}']) {
        bases.push(json_go(text));
    }
    let mut levels = Vec::new();
    for base in bases {
        if !text.is_ascii() {
            levels.push(ascii_escaped(&base, false));
            levels.push(ascii_escaped(&base, true));
        }
        if text.contains(HTML_UNSAFE) {
            levels.push(html_escaped(&base));
        }
        levels.push(base);
    }
    let slashed: Vec<String> = levels
        .iter()
        .filter(|level| level.contains('/'))
        .map(|level| level.replace('/', "\\/"))
        .collect();
    levels.extend(slashed);
    for level in levels {
        let doubled = json_inner(&level);
        if doubled != level {
            into.push(doubled);
        }
        into.push(level);
    }
}

fn with_trimmed(text: &str, into: &mut Vec<String>) {
    forms_of(text, into);
    let trimmed = text.trim();
    if trimmed.len() != text.len() && trimmed.len() >= MIN_HIDDEN {
        forms_of(trimmed, into);
    }
}

pub fn hide_forms(value: &str, mask: &str) -> BTreeMap<String, String> {
    let mut forms = Vec::new();
    with_trimmed(value, &mut forms);
    if value.contains(['\n', '\r']) {
        for line in value.split(['\n', '\r']) {
            if line.len() >= MIN_HIDDEN && !line.trim().is_empty() {
                with_trimmed(line, &mut forms);
            }
        }
    }
    let mut hidden = BTreeMap::new();
    for form in forms {
        if form.is_empty() || hidden.contains_key(&form) {
            wipe_string(form);
        } else {
            hidden.insert(form, mask.to_string());
        }
    }
    hidden
}
