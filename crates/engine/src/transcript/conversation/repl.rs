use crate::transcript::util::str_of;
use serde_json::Value;
use std::collections::BTreeMap;

const TAGS: [&str; 4] = ["log", "result", "error", "refused"];
const KEEP_EVALS: usize = 256;
const TAG_SCAN: usize = 1024;

pub(super) fn is_inner(id: &str) -> bool {
    id.starts_with("repl_") || id.starts_with("toolu_cc_")
}

pub(super) fn is_repl(name: &str) -> bool {
    name == "REPL"
}

pub(super) struct Event {
    pub eval: u64,
    pub failed: bool,
    pub closed: bool,
    pub text: String,
}

#[derive(Default)]
pub(super) struct Repl {
    by_eval: BTreeMap<u64, String>,
    polled: Vec<String>,
    last: Option<String>,
}

impl Repl {
    pub(super) fn started(&mut self, id: &str) {
        self.last = Some(id.to_string());
        self.polled.clear();
    }

    pub(super) fn left_script(&mut self) {
        self.last = None;
        self.polled.clear();
    }

    pub(super) fn dispatched(&mut self, eval: u64, id: &str) {
        self.by_eval.insert(eval, id.to_string());
    }

    pub(super) fn polled(&mut self, evals: &[u64]) {
        self.polled = evals
            .iter()
            .filter_map(|eval| self.by_eval.remove(eval))
            .collect();
    }

    pub(super) fn call_of(&self, eval: u64) -> Option<&str> {
        self.by_eval.get(&eval).map(String::as_str)
    }

    pub(super) fn parent_for_inner(&self) -> Option<String> {
        self.polled.last().cloned().or_else(|| self.last.clone())
    }

    pub(super) fn trim(&mut self, live: impl Fn(&str) -> bool) {
        self.by_eval.retain(|_, id| live(id));
        while self.by_eval.len() > KEEP_EVALS {
            self.by_eval.pop_first();
        }
    }
}

pub(super) fn dispatched_eval(result: Option<&Value>) -> Option<u64> {
    let result = result?;
    if result.get("asyncDispatched").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    result.get("evalId").and_then(Value::as_u64)
}

fn tag_end(text: &str) -> usize {
    let mut quote = false;
    for (at, byte) in text.bytes().enumerate() {
        match byte {
            b'"' => quote = !quote,
            b'>' if !quote => return at,
            _ => {}
        }
    }
    text.len()
}

fn opens(text: &str, name: &str) -> bool {
    text.strip_prefix('<')
        .and_then(|t| t.strip_prefix(name))
        .is_some_and(|t| t.starts_with(char::is_whitespace))
}

pub(super) fn is_event_text(text: &str) -> bool {
    let text = text.trim_start();
    if !opens(text, "event") {
        return false;
    }
    let mut scan = text.len().min(TAG_SCAN);
    while !text.is_char_boundary(scan) {
        scan -= 1;
    }
    let tag = &text[..tag_end(&text[..scan])];
    tag.contains("kind=\"repl-")
}

pub(super) fn envelopes(attachment: &Value) -> Vec<Event> {
    attachment
        .get("envelopes")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .flat_map(events)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn queued_event(obj: &Value) -> Vec<Event> {
    str_of(obj, "content").map(events).unwrap_or_default()
}

fn open_tag(text: &str) -> Option<&'static str> {
    let rest = text.strip_prefix('<')?;
    TAGS.into_iter().find(|tag| {
        rest.strip_prefix(*tag)
            .is_some_and(|after| after.starts_with('>'))
    })
}

enum Child<'a> {
    Tag(&'static str, &'a str),
    Json(Value),
    CutJson(&'a str),
}

fn children(body: &str) -> (Vec<Child<'_>>, usize, bool) {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let tail = &body[pos..];
        let trimmed = tail.trim_start();
        pos += tail.len() - trimmed.len();
        if trimmed.is_empty() {
            return (out, pos, false);
        }
        if trimmed.starts_with("</event") {
            let end = tag_end(trimmed);
            return (out, (pos + end + 1).min(body.len()), true);
        }
        if let Some(tag) = open_tag(trimmed) {
            let from = pos + tag.len() + 2;
            let close = format!("</{tag}>");
            match body[from..].find(&close) {
                Some(len) => {
                    out.push(Child::Tag(tag, &body[from..from + len]));
                    pos = from + len + close.len();
                }
                None => {
                    out.push(Child::Tag(tag, &body[from..]));
                    return (out, body.len(), false);
                }
            }
            continue;
        }
        if trimmed.starts_with('{') {
            let mut stream = serde_json::Deserializer::from_str(trimmed).into_iter::<Value>();
            if let Some(Ok(value)) = stream.next() {
                out.push(Child::Json(value));
                pos += stream.byte_offset();
                continue;
            }
            out.push(Child::CutJson(trimmed));
            return (out, body.len(), false);
        }
        let skip = trimmed.chars().next().map_or(1, char::len_utf8);
        match body[pos + skip..].find('<') {
            Some(len) => pos += skip + len,
            None => return (out, body.len(), false),
        }
    }
}

pub(super) fn events(text: &str) -> Vec<Event> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("<event") {
        let after = &rest[at..];
        if !opens(after, "event") {
            rest = &after[6..];
            continue;
        }
        let head_end = tag_end(after);
        let head = &after[..head_end];
        let from = (head_end + 1).min(after.len());
        let (kids, used, closed) = children(&after[from..]);
        if attr(head, "kind") == Some("repl-eval") {
            if let Some(event) = event(head, &kids, closed) {
                out.push(event);
            }
        }
        rest = &after[from + used..];
    }
    out
}

fn event(head: &str, kids: &[Child<'_>], closed: bool) -> Option<Event> {
    let mut eval = attr(head, "eval").and_then(|v| v.parse::<u64>().ok());
    let mut failed = attr(head, "status").is_some_and(|s| s != "settled");
    let mut parts: Vec<String> = Vec::new();
    for kid in kids {
        match kid {
            Child::Tag(tag, text) => {
                if matches!(*tag, "error" | "refused") {
                    failed = true;
                }
                let text = unescape(text.trim());
                if !text.is_empty() {
                    parts.push(text);
                }
            }
            Child::CutJson(raw) => eval = eval.or_else(|| cut_eval(raw)),
            Child::Json(value) => {
                eval = eval.or_else(|| value.get("eval").and_then(Value::as_u64));
                for key in ["log", "result", "error", "refused"] {
                    let Some(found) = value.get(key).filter(|v| !v.is_null()) else {
                        continue;
                    };
                    if matches!(key, "error" | "refused") {
                        failed = true;
                    }
                    for line in json_lines(found) {
                        if !line.is_empty() {
                            parts.push(line);
                        }
                    }
                }
            }
        }
    }
    Some(Event {
        eval: eval?,
        failed,
        closed,
        text: parts.join("\n"),
    })
}

fn cut_eval(raw: &str) -> Option<u64> {
    let after = raw.trim_start().strip_prefix('{')?.trim_start();
    let after = after.strip_prefix("\"eval\":")?.trim_start();
    let digits = after.bytes().take_while(u8::is_ascii_digit).count();
    let next = after.as_bytes().get(digits)?;
    if digits == 0 || !matches!(next, b',' | b'}' | b' ' | b'\n' | b'\r' | b'\t') {
        return None;
    }
    after.get(..digits)?.parse().ok()
}

fn json_lines(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) => vec![s.clone()],
        Value::Array(list) => list.iter().flat_map(json_lines).collect(),
        other => vec![other.to_string()],
    }
}

fn attr<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let from = head.find(&key)? + key.len();
    let len = head[from..].find('"')?;
    Some(&head[from..from + len])
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let decoded = tail
            .bytes()
            .take(10)
            .position(|b| b == b';')
            .and_then(|end| entity(&tail[..end]).map(|c| (c, end)));
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    match name {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let digits = name.strip_prefix('#')?;
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

fn hex4(text: &str) -> Option<u32> {
    text.get(..4)
        .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
        .and_then(|h| u32::from_str_radix(h, 16).ok())
}

fn decode_string(text: &str) -> (String, usize, bool) {
    let mut out = String::new();
    let mut pos = 0;
    while let Some(c) = text[pos..].chars().next() {
        pos += c.len_utf8();
        match c {
            '"' => return (out, pos, true),
            '\\' => {
                let Some(e) = text[pos..].chars().next() else {
                    return (out, text.len(), false);
                };
                pos += e.len_utf8();
                match e {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'u' => {
                        let rest = &text[pos..];
                        let Some(unit) = hex4(rest) else {
                            if rest.len() < 4 {
                                return (out, text.len(), false);
                            }
                            out.push('\u{fffd}');
                            continue;
                        };
                        pos += 4;
                        if (0xD800..0xDC00).contains(&unit) {
                            let rest = &text[pos..];
                            let low = rest
                                .strip_prefix("\\u")
                                .and_then(hex4)
                                .filter(|l| (0xDC00..0xE000).contains(l));
                            match low {
                                Some(low) => {
                                    pos += 6;
                                    let code = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                                }
                                None if "\\u".starts_with(rest)
                                    || (rest.starts_with("\\u") && rest.len() < 6) =>
                                {
                                    return (out, text.len(), false);
                                }
                                None => out.push('\u{fffd}'),
                            }
                        } else {
                            out.push(char::from_u32(unit).unwrap_or('\u{fffd}'));
                        }
                    }
                    other => out.push(other),
                }
            }
            other => out.push(other),
        }
    }
    (out, text.len(), false)
}

pub(super) fn oversized_events(head: &[u8]) -> (Vec<Event>, bool) {
    let text = String::from_utf8_lossy(head);
    if text.contains(r#""type":"queue-operation""#) && text.contains(r#""operation":"enqueue""#) {
        let Some((_, rest)) = text.split_once(r#""content":""#) else {
            return (Vec::new(), false);
        };
        let (content, _, _) = decode_string(rest);
        if !is_event_text(&content) {
            return (Vec::new(), false);
        }
        return (events(&content), false);
    }
    if !text.contains(r#""type":"poll_events""#) {
        return (Vec::new(), false);
    }
    let Some((_, mut rest)) = text.split_once(r#""envelopes":["#) else {
        return (Vec::new(), true);
    };
    let mut out = Vec::new();
    while let Some(body) = rest.trim_start_matches([' ', ',', '\n']).strip_prefix('"') {
        let (envelope, used, done) = decode_string(body);
        out.extend(events(&envelope));
        if !done {
            break;
        }
        rest = &body[used..];
    }
    (out, true)
}

pub(super) fn title(description: &str) -> String {
    if description.trim().is_empty() {
        "Ran a script".to_string()
    } else {
        description.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_poll_forgets_the_evals_it_settles_and_keeps_their_calls_for_inner_rows() {
        let mut repl = Repl::default();
        repl.dispatched(1, "a");
        repl.dispatched(2, "b");
        repl.polled(&[1]);
        assert!(repl.call_of(1).is_none());
        assert_eq!(repl.call_of(2), Some("b"));
        assert_eq!(repl.parent_for_inner().as_deref(), Some("a"));
    }

    #[test]
    fn trim_drops_dead_calls_and_keeps_only_the_newest_evals() {
        let mut repl = Repl::default();
        for eval in 0..300 {
            repl.dispatched(eval, &format!("c{eval}"));
        }
        repl.trim(|id| id != "c299");
        assert_eq!(repl.by_eval.len(), KEEP_EVALS);
        assert!(repl.call_of(299).is_none());
        assert!(repl.call_of(298).is_some());
        assert!(repl.call_of(0).is_none());
    }

    #[test]
    fn a_script_left_clears_the_last_call() {
        let mut repl = Repl::default();
        repl.started("a");
        repl.left_script();
        assert!(repl.parent_for_inner().is_none());
    }

    fn head(tail: &str) -> Vec<u8> {
        tail.as_bytes().to_vec()
    }

    #[test]
    fn an_oversized_poll_reads_every_envelope_in_its_head() {
        let line = head(
            r#"{"type":"attachment","attachment":{"type":"poll_events","envelopes":["<event kind=\"repl-status\">x</event>","<event kind=\"repl-eval\" eval=\"1\" status=\"settled\">\n<result>one</result>\n</event>","<event kind=\"repl-eval\" eval=\"2\" status=\"settled\">\n<result>two "#,
        );
        let (events, polled) = oversized_events(&line);
        assert!(polled);
        let seen: Vec<(u64, &str, bool)> = events
            .iter()
            .map(|e| (e.eval, e.text.as_str(), e.closed))
            .collect();
        assert_eq!(seen, [(1, "one", true), (2, "two", false)]);
    }

    fn cut_event(body: &str) -> Vec<Event> {
        let line = head(&format!(
            r#"{{"type":"queue-operation","operation":"enqueue","content":"<event kind=\"repl-eval\" status=\"settled\">\n{body}"#
        ));
        oversized_events(&line).0
    }

    #[test]
    fn a_cut_json_body_names_its_eval_and_is_not_failed() {
        let events = cut_event(r#"{\"eval\":4,\"log\":null,\"error\":\"boom "#);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].eval, 4);
        assert!(!events[0].failed);
        assert!(!events[0].closed);
        assert!(events[0].text.is_empty());
    }

    #[test]
    fn a_cut_null_does_not_read_as_an_error() {
        for tail in ["n", "nu", "nul", "null"] {
            let events = cut_event(&format!(r#"{{\"eval\":4,\"error\":{tail}"#));
            assert!(!events[0].failed, "{tail}");
        }
    }

    #[test]
    fn an_error_key_inside_a_nested_result_does_not_fail_the_script() {
        let events = cut_event(r#"{\"eval\":4,\"result\":{\"error\":\"x\""#);
        assert!(!events[0].failed);
    }

    #[test]
    fn a_cut_eval_number_settles_nothing_and_a_whole_one_does() {
        assert!(cut_event(r#"{\"eval\":12"#).is_empty());
        assert!(cut_event(r#"{\"eval\":"#).is_empty());
        assert_eq!(cut_event(r#"{\"eval\":12,\"lo"#)[0].eval, 12);
        assert_eq!(cut_event(r#"{\"eval\":12}"#)[0].eval, 12);
        assert_eq!(cut_event("{\\\"eval\\\": 12 ")[0].eval, 12);
    }

    #[test]
    fn an_eval_key_nested_before_the_top_level_one_settles_nothing() {
        assert!(cut_event(r#"{\"result\":{\"eval\":9},\"eval\":4,\"lo"#).is_empty());
        assert!(cut_event(r#"{\"log\":\"x \\\"eval\\\":9, y"#).is_empty());
        assert_eq!(
            cut_event(r#"  {  \"eval\":4,\"result\":{\"eval\":9},"#)[0].eval,
            4
        );
    }

    #[test]
    fn a_high_surrogate_before_a_multibyte_character_does_not_panic() {
        let line = head(
            r#"{"type":"queue-operation","operation":"enqueue","content":"<event kind=\"repl-eval\" eval=\"1\" status=\"settled\">\n<result>ab\ud83d€"#,
        );
        let (events, _) = oversized_events(&line);
        assert_eq!(events[0].text, "ab\u{fffd}€");
    }

    #[test]
    fn a_surrogate_pair_decodes_to_one_character() {
        let line = head(
            r#"{"type":"queue-operation","operation":"enqueue","content":"<event kind=\"repl-eval\" eval=\"1\" status=\"settled\">\n<result>go \ud83d\ude00 end"#,
        );
        let (events, _) = oversized_events(&line);
        assert_eq!(events[0].text, "go \u{1f600} end");
    }

    #[test]
    fn an_escape_cut_by_the_head_is_dropped() {
        for cut in [r"\u004", r"\ud83d", r"\ud83d\ude0", r"\ud83d\", "\\"] {
            let text = format!(
                r#"{{"type":"queue-operation","operation":"enqueue","content":"<event kind=\"repl-eval\" eval=\"1\" status=\"settled\">\n<result>ab{cut}"#
            );
            let (events, _) = oversized_events(&head(&text));
            assert_eq!(events[0].text, "ab", "{cut}");
        }
    }

    #[test]
    fn an_oversized_prompt_that_quotes_an_event_settles_nothing() {
        let line = head(
            r#"{"type":"queue-operation","operation":"enqueue","content":"look at <event kind=\"repl-eval\" eval=\"1\" status=\"failed\">\n<error>x</error>\n</event> please"#,
        );
        assert!(oversized_events(&line).0.is_empty());
    }
}
