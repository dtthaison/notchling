use super::runner::{Channel, OutputSink};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const WITHHELD: &str = "[sign-in output withheld]";

const LINK_MAX: usize = 300;
const LONG_LINE: usize = 4096;
const PAUSE: Duration = Duration::from_millis(250);
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
}

impl Scanner {
    pub fn line(&mut self, raw: &str) -> Option<DevicePrompt> {
        let line = strip_ansi(raw);
        let text = line.trim();
        if text.is_empty() {
            return self.prompt();
        }
        let has_link = text.contains("://");
        let marker = text.split_whitespace().any(is_marker);
        if has_link {
            for token in text.split_whitespace().map(trimmed) {
                if self.url.is_none() && is_device_link(token) {
                    self.url = Some(token.to_owned());
                }
            }
        }
        if self.code.is_none() {
            if marker {
                let bare: Vec<&str> = text
                    .split_whitespace()
                    .filter(|t| !t.contains("://"))
                    .collect();
                match after_label(&bare.join(" ")) {
                    Some(code) => {
                        self.code = Some(code);
                        self.expect_code = false;
                    }
                    None => self.expect_code = true,
                }
            } else if has_link {
                self.expect_code = false;
            } else if self.expect_code {
                self.code = single_code(text);
                self.expect_code = false;
            }
        }
        self.prompt()
    }

    fn prompt(&self) -> Option<DevicePrompt> {
        match (&self.url, &self.code) {
            (Some(url), Some(code)) => DevicePrompt::new(url, code),
            _ => None,
        }
    }
}

struct Inner {
    sink: Arc<OutputSink>,
    prompt: PromptSink,
    scanner: Mutex<Scanner>,
    partial: Mutex<BTreeMap<Channel, String>>,
    offered: Mutex<Option<DevicePrompt>>,
    withheld: Mutex<bool>,
    seq: AtomicU64,
}

pub struct DeviceFilter {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl DeviceFilter {
    pub fn new(sink: Arc<OutputSink>, prompt: PromptSink) -> DeviceFilter {
        DeviceFilter {
            inner: Arc::new(Inner {
                sink,
                prompt,
                scanner: Mutex::new(Scanner::default()),
                partial: Mutex::new(BTreeMap::new()),
                offered: Mutex::new(None),
                withheld: Mutex::new(false),
                seq: AtomicU64::new(0),
            }),
        }
    }

    pub fn feed(&self, channel: Channel, chunk: &str) {
        let seq = self.inner.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let mut lines = Vec::new();
        let rest;
        {
            let mut partial = lock(&self.inner.partial);
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
            self.inner.emit(channel, &line);
        }
        if !rest.trim().is_empty() {
            let inner = self.inner.clone();
            std::thread::spawn(move || {
                std::thread::sleep(PAUSE);
                if inner.seq.load(Ordering::SeqCst) == seq {
                    let peeked = lock(&inner.scanner).clone().line(&rest);
                    inner.offer(peeked);
                }
            });
        }
    }

    pub fn finish(&self) {
        self.inner.seq.fetch_add(1, Ordering::SeqCst);
        let rest = std::mem::take(&mut *lock(&self.inner.partial));
        for (channel, text) in rest {
            if !text.trim().is_empty() {
                self.inner.emit(channel, &format!("{text}\n"));
            }
        }
    }
}

impl Inner {
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
        let found = lock(&self.scanner).line(line);
        self.offer(found);
        let first = !std::mem::replace(&mut *lock(&self.withheld), true);
        if first {
            (self.sink)(channel, &format!("{WITHHELD}\n"));
        }
    }
}
