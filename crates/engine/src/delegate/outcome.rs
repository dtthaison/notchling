//! What a delegated run came to, and how it is printed (Swift `Delegate.Outcome`, `render`,
//! `json`). Text and `--json` shape and the exit codes are the Swift command's.

use super::options::Tool;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub tool: Tool,
    pub session: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub reply: String,
    /// Images, videos and audio the tool made, where it saved them.
    pub media: Vec<String>,
    /// Where `--out` copied them.
    pub copied: Vec<String>,
    /// Permissions the tool asked for that were declined (agy), by title.
    pub declined: Vec<String>,
    pub error: Option<String>,
    pub timed_out: bool,
    pub seconds: f64,
    pub tokens: Option<u64>,
}

impl Outcome {
    pub fn new(tool: Tool) -> Outcome {
        Outcome {
            tool,
            session: None,
            model: None,
            effort: None,
            reply: String::new(),
            media: Vec::new(),
            copied: Vec::new(),
            declined: Vec::new(),
            error: None,
            timed_out: false,
            seconds: 0.0,
            tokens: None,
        }
    }

    /// 0 when it finished, 1 when it failed, 124 on timeout.
    pub fn exit_code(&self) -> i32 {
        if self.timed_out {
            124
        } else if self.error.is_none() {
            0
        } else {
            1
        }
    }
}

/// The text form: a header line, `Session:`, `Error:`, the files, the declined permissions, then
/// the reply after a blank line. No trailing newline (the caller prints one).
pub fn render(o: &Outcome) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut head = o.tool.title().to_string();
    let pick: Vec<&str> = [o.model.as_deref(), o.effort.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    if !pick.is_empty() {
        head.push_str(&format!(" ({})", pick.join(", ")));
    }
    head.push_str(if o.timed_out {
        ": timed out"
    } else if o.error.is_none() {
        ": done"
    } else {
        ": failed"
    });
    head.push_str(&format!(" in {:.0}s", o.seconds));
    if let Some(t) = o.tokens {
        head.push_str(&format!(", {t} tokens"));
    }
    lines.push(head);
    if let Some(s) = &o.session {
        lines.push(format!("Session: {s}"));
    }
    if let Some(e) = &o.error {
        lines.push(format!("Error: {e}"));
    }
    for (title, list) in [
        ("Generated:", &o.media),
        ("Copied to:", &o.copied),
        ("Declined (not allowed in a delegated run):", &o.declined),
    ] {
        if !list.is_empty() {
            lines.push(title.to_string());
            lines.extend(list.iter().map(|p| format!("  {p}")));
        }
    }
    if !o.reply.is_empty() {
        lines.push(String::new());
        lines.push(o.reply.clone());
    }
    lines.join("\n")
}

/// The `--json` form: one line, keys sorted (whatever features serde_json is built with), slashes unescaped.
pub fn to_json(o: &Outcome) -> String {
    let mut d: BTreeMap<&str, serde_json::Value> = BTreeMap::new();
    d.insert("tool", json!(o.tool.raw()));
    d.insert("ok", json!(o.error.is_none()));
    d.insert("seconds", json!(o.seconds.round() as i64));
    d.insert("reply", json!(o.reply));
    d.insert("media", json!(o.media));
    d.insert("copied", json!(o.copied));
    d.insert("declined", json!(o.declined));
    for (key, value) in [
        ("session", &o.session),
        ("model", &o.model),
        ("effort", &o.effort),
        ("error", &o.error),
    ] {
        if let Some(v) = value {
            d.insert(key, json!(v));
        }
    }
    if let Some(t) = o.tokens {
        d.insert("tokens", json!(t));
    }
    if o.timed_out {
        d.insert("timedOut", json!(true));
    }
    serde_json::to_string(&d).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn done() -> Outcome {
        let mut o = Outcome::new(Tool::Codex);
        o.session = Some("s1".into());
        o.model = Some("m".into());
        o.effort = Some("high".into());
        o.reply = "Done.".into();
        o.media = vec!["/a.png".into()];
        o.seconds = 42.0;
        o
    }

    #[test]
    fn it_renders_the_swift_text_and_exit_codes() {
        let mut o = done();
        let text = render(&o);
        assert!(text.starts_with("Codex (m, high): done in 42s"));
        assert!(text.contains("Session: s1\nGenerated:\n  /a.png"));
        assert!(text.ends_with("\n\nDone."));
        assert_eq!(o.exit_code(), 0);
        o.error = Some("x".into());
        assert_eq!(o.exit_code(), 1);
        assert!(render(&o).contains("failed in 42s"));
        assert!(render(&o).contains("Session: s1\nError: x\nGenerated:"));
        o.timed_out = true;
        assert_eq!(o.exit_code(), 124);
        assert!(render(&o).starts_with("Codex (m, high): timed out in 42s"));
        assert!(to_json(&o).contains("\"media\":[\"/a.png\"]"));
    }

    #[test]
    fn every_section_has_its_title_and_the_header_has_tokens() {
        let mut o = Outcome::new(Tool::Agy);
        o.tokens = Some(900);
        o.seconds = 1.5;
        o.copied = vec!["/o/a.png".into()];
        o.declined = vec!["Run ls".into()];
        let text = render(&o);
        assert_eq!(
            text,
            "Antigravity CLI: done in 2s, 900 tokens\nCopied to:\n  /o/a.png\nDeclined (not allowed in a delegated run):\n  Run ls"
        );
    }

    #[test]
    fn json_has_sorted_keys_and_only_what_is_set() {
        let mut o = Outcome::new(Tool::Agy);
        o.reply = "a/b é".into();
        o.seconds = 12.4;
        assert_eq!(
            to_json(&o),
            r#"{"copied":[],"declined":[],"media":[],"ok":true,"reply":"a/b é","seconds":12,"tool":"agy"}"#
        );
        o.error = Some("boom".into());
        o.timed_out = true;
        o.session = Some("c".into());
        o.model = Some("m".into());
        o.effort = Some("low".into());
        o.tokens = Some(7);
        let v: Value = serde_json::from_str(&to_json(&o)).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["timedOut"], true);
        assert_eq!(v["error"], "boom");
        assert_eq!(v["tokens"], 7);
        assert_eq!(v["session"], "c");
        assert_eq!(v["model"], "m");
        assert_eq!(v["effort"], "low");
        assert!(!to_json(&o).contains('\n'));
    }
}
