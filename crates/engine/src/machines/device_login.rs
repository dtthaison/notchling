use super::runner::{Channel, OutputSink};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

pub const HIDDEN_LINK: &str = "[sign-in link]";
pub const HIDDEN_CODE: &str = "[one-time code]";

const LINK_MAX: usize = 300;
const LONG_LINE: usize = 4096;
const CODE_PART: std::ops::RangeInclusive<usize> = 3..=8;
const HOSTS: [&str; 2] = ["openai.com", "chatgpt.com"];

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
                for next in chars.by_ref() {
                    if next == '\u{7}' {
                        break;
                    }
                }
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
    let Some((left, right)) = token.split_once('-') else {
        return false;
    };
    let part = |s: &str| {
        CODE_PART.contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    };
    part(left) && part(right)
}

fn trimmed(token: &str) -> &str {
    token.trim_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | ')' | '(' | '"' | '\''))
}

#[derive(Default)]
pub struct Scanner {
    url: Option<String>,
    code: Option<String>,
}

impl Scanner {
    pub fn line(&mut self, raw: &str) -> Option<DevicePrompt> {
        let line = strip_ansi(raw);
        if line.len() > 200 {
            return None;
        }
        for token in line.split_whitespace().map(trimmed) {
            if self.url.is_none() && is_device_link(token) {
                self.url = Some(token.to_owned());
            } else if self.url.is_some() && self.code.is_none() && is_device_code(token) {
                self.code = Some(token.to_owned());
            }
        }
        match (&self.url, &self.code) {
            (Some(url), Some(code)) => DevicePrompt::new(url, code),
            _ => None,
        }
    }

    pub fn scrub(&self, text: &str) -> String {
        let plain = strip_ansi(text);
        let mut out = String::with_capacity(plain.len());
        let mut token = String::new();
        let flush = |token: &mut String, out: &mut String| {
            if token.is_empty() {
                return;
            }
            out.push_str(&self.hide(token));
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

    fn hide(&self, token: &str) -> String {
        let core = trimmed(token);
        let known_url = self.url.as_deref().is_some_and(|u| token.contains(u));
        let known_code = self.code.as_deref().is_some_and(|c| token.contains(c));
        if known_url || core.contains("://") {
            return HIDDEN_LINK.to_owned();
        }
        if known_code || is_device_code(core) {
            return HIDDEN_CODE.to_owned();
        }
        token.to_owned()
    }
}

pub struct DeviceFilter {
    sink: Arc<OutputSink>,
    prompt: PromptSink,
    scanner: Mutex<Scanner>,
    partial: Mutex<BTreeMap<Channel, String>>,
    offered: Mutex<bool>,
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
            offered: Mutex::new(false),
        }
    }

    pub fn feed(&self, channel: Channel, chunk: &str) {
        let mut lines = Vec::new();
        {
            let mut partial = lock(&self.partial);
            let buf = partial.entry(channel).or_default();
            buf.push_str(chunk);
            while let Some(at) = buf.find('\n') {
                lines.push(buf.drain(..=at).collect::<String>());
            }
            if buf.len() > LONG_LINE {
                buf.clear();
                lines.push("[output line cut]\n".into());
            }
        }
        for line in lines {
            self.emit(channel, &line);
        }
    }

    pub fn finish(&self) {
        let rest = std::mem::take(&mut *lock(&self.partial));
        for (channel, text) in rest {
            if !text.is_empty() {
                self.emit(channel, &format!("{text}\n"));
            }
        }
    }

    pub fn scrub(&self, text: &str) -> String {
        lock(&self.scanner).scrub(text)
    }

    fn emit(&self, channel: Channel, line: &str) {
        let (found, shown) = {
            let mut scanner = lock(&self.scanner);
            let found = scanner.line(line);
            (found, scanner.scrub(line))
        };
        if let Some(found) = found {
            let first = {
                let mut offered = lock(&self.offered);
                !std::mem::replace(&mut *offered, true)
            };
            if first {
                (self.prompt)(found);
            }
        }
        (self.sink)(channel, &shown);
    }
}
