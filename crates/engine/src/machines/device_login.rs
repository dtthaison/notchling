use super::runner::{Channel, OutputSink};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

pub const HIDDEN_LINK: &str = "[sign-in link]";
pub const WITHHELD: &str = "[sign-in output withheld]";

const LINK_MAX: usize = 300;
const LONG_LINE: usize = 4096;
const CODE_LEN: std::ops::RangeInclusive<usize> = 4..=40;
const HOSTS: [&str; 2] = ["openai.com", "chatgpt.com"];
const WRAP: &[char] = &[
    '.', ',', ';', ':', ')', '(', '"', '\'', '`', '[', ']', '<', '>', '{', '}', '*', '_',
];

#[derive(Clone, PartialEq, Eq)]
pub struct DevicePrompt {
    url: String,
    code: String,
}

impl DevicePrompt {
    pub fn new(url: &str, code: &str) -> Option<DevicePrompt> {
        (is_device_link(url) && is_device_code(code)).then(|| DevicePrompt {
            url: url.to_owned(),
            code: code.to_owned(),
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Debug for DevicePrompt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevicePrompt").finish_non_exhaustive()
    }
}

pub type PromptSink = Arc<dyn Fn(DevicePrompt) + Send + Sync>;

pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                let mut body = String::new();
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' {
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                    body.push(next);
                }
                if let Some(uri) = body.strip_prefix("8;").and_then(|b| b.split_once(';')) {
                    if !uri.1.is_empty() {
                        out.push(' ');
                        out.push_str(uri.1);
                        out.push(' ');
                    }
                }
            }
            Some('(' | ')' | '*' | '+') => {
                chars.next();
                chars.next();
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

pub fn is_device_link(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host_ok = HOSTS
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")));
    url.len() <= LINK_MAX
        && host_ok
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && path.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '/' | '-' | '_' | '.' | '~' | '?' | '=' | '&' | '%')
        })
}

pub fn is_device_code(token: &str) -> bool {
    CODE_LEN.contains(&token.len())
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !token.starts_with('-')
        && !token.ends_with('-')
        && (token.contains('-')
            || token.chars().any(|c| c.is_ascii_digit())
            || token.chars().all(|c| c.is_ascii_uppercase()))
}

fn trimmed(token: &str) -> &str {
    token.trim_matches(WRAP)
}

fn single_code(text: &str) -> Option<String> {
    let mut words = text.split_whitespace();
    let word = words.next()?;
    if words.next().is_some() {
        return None;
    }
    let word = word.rsplit(['=', ':']).next().unwrap_or(word);
    let word = trimmed(word);
    is_device_code(word).then(|| word.to_owned())
}

fn after_label(text: &str) -> Option<String> {
    let at = text.rfind([':', '='])?;
    single_code(&text[at + 1..])
}

fn is_marker(token: &str) -> bool {
    matches!(
        trimmed(token).to_ascii_lowercase().as_str(),
        "code" | "codes" | "one-time" | "device-auth"
    )
}

#[derive(Clone, Default)]
pub struct Scanner {
    url: Option<String>,
    code: Option<String>,
    expect_code: bool,
    triggered: bool,
}

impl Scanner {
    pub fn triggered(&self) -> bool {
        self.triggered
    }

    pub fn line(&mut self, raw: &str) -> Option<DevicePrompt> {
        let line = strip_ansi(raw);
        let text = line.trim();
        if text.is_empty() {
            return self.prompt();
        }
        let has_link = text.contains("://");
        let marker = text.split_whitespace().any(is_marker);
        if has_link || marker {
            self.triggered = true;
        }
        if has_link {
            for token in text.split_whitespace().map(trimmed) {
                if self.url.is_none() && is_device_link(token) {
                    self.url = Some(token.to_owned());
                }
            }
        }
        if self.expect_code && !has_link && !marker {
            if let Some(code) = single_code(text) {
                self.code = Some(code);
            }
            self.expect_code = false;
        } else if marker && !has_link {
            self.expect_code = true;
            if let Some(code) = after_label(text) {
                self.code = Some(code);
                self.expect_code = false;
            }
        } else {
            self.expect_code = false;
        }
        self.prompt()
    }

    fn prompt(&self) -> Option<DevicePrompt> {
        match (&self.url, &self.code) {
            (Some(url), Some(code)) => DevicePrompt::new(url, code),
            _ => None,
        }
    }

    pub fn scrub(&self, text: &str) -> String {
        if self.triggered {
            return WITHHELD.to_owned();
        }
        let plain = strip_ansi(text);
        let mut out = String::with_capacity(plain.len());
        let mut token = String::new();
        let flush = |token: &mut String, out: &mut String| {
            if token.is_empty() {
                return;
            }
            if token.contains("://") {
                out.push_str(HIDDEN_LINK);
            } else {
                out.push_str(token);
            }
            token.clear();
        };
        for c in plain.chars() {
            if c.is_whitespace() {
                flush(&mut token, &mut out);
                out.push(c);
            } else {
                token.push(c);
            }
        }
        flush(&mut token, &mut out);
        out
    }
}

pub struct DeviceFilter {
    sink: Arc<OutputSink>,
    prompt: PromptSink,
    scanner: Mutex<Scanner>,
    partial: Mutex<BTreeMap<Channel, String>>,
    offered: Mutex<Option<DevicePrompt>>,
    withheld: Mutex<bool>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl DeviceFilter {
    pub fn new(sink: Arc<OutputSink>, prompt: PromptSink) -> DeviceFilter {
        DeviceFilter {
            sink,
            prompt,
            scanner: Mutex::new(Scanner::default()),
            partial: Mutex::new(BTreeMap::new()),
            offered: Mutex::new(None),
            withheld: Mutex::new(false),
        }
    }

    pub fn feed(&self, channel: Channel, chunk: &str) {
        let mut lines = Vec::new();
        let rest;
        {
            let mut partial = lock(&self.partial);
            let buf = partial.entry(channel).or_default();
            buf.push_str(chunk);
            while let Some(at) = buf.find(['\n', '\r']) {
                let line: String = buf.drain(..=at).collect();
                if !line.trim().is_empty() {
                    lines.push(line);
                }
            }
            if buf.len() > LONG_LINE {
                buf.clear();
                lines.push("[output line cut]\n".into());
            }
            rest = buf.clone();
        }
        for line in lines {
            self.emit(channel, &line);
        }
        if !rest.trim().is_empty() {
            let peeked = lock(&self.scanner).clone().line(&rest);
            self.offer(peeked);
        }
    }

    pub fn finish(&self) {
        let rest = std::mem::take(&mut *lock(&self.partial));
        for (channel, text) in rest {
            if !text.trim().is_empty() {
                self.emit(channel, &format!("{text}\n"));
            }
        }
    }

    pub fn triggered(&self) -> bool {
        lock(&self.scanner).triggered()
    }

    pub fn scrub(&self, text: &str) -> String {
        let scanner = lock(&self.scanner);
        if scanner.triggered() {
            String::new()
        } else {
            scanner.scrub(text)
        }
    }

    fn offer(&self, found: Option<DevicePrompt>) {
        let Some(found) = found else {
            return;
        };
        let changed = {
            let mut offered = lock(&self.offered);
            if offered.as_ref() == Some(&found) {
                false
            } else {
                *offered = Some(found.clone());
                true
            }
        };
        if changed {
            (self.prompt)(found);
        }
    }

    fn emit(&self, channel: Channel, line: &str) {
        let (found, hidden, shown) = {
            let mut scanner = lock(&self.scanner);
            let found = scanner.line(line);
            (found, scanner.triggered(), scanner.scrub(line))
        };
        self.offer(found);
        if hidden {
            let first = !std::mem::replace(&mut *lock(&self.withheld), true);
            if first {
                (self.sink)(channel, &format!("{WITHHELD}\n"));
            }
            return;
        }
        (self.sink)(channel, &shown);
    }
}
