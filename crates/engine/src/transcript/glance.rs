use super::model::{Conversation, ItemKind};
use super::tail::PARSED_BYTES;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::Ordering;

pub const FIRST: u64 = 64 * 1024;
pub const CAP: u64 = 1024 * 1024;

const WINDOWS: [u64; 3] = [FIRST, FIRST * 4, CAP];

pub struct Window {
    bytes: Vec<u8>,
    whole: bool,
}

impl Window {
    pub fn read(path: &str, len: u64) -> io::Result<Window> {
        let mut file = File::open(path)?;
        let size = file.seek(SeekFrom::End(0))?;
        let start = size.saturating_sub(len);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::with_capacity((size - start) as usize);
        file.take(size - start).read_to_end(&mut bytes)?;
        PARSED_BYTES.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        Ok(Window {
            bytes,
            whole: start == 0,
        })
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn whole(&self) -> bool {
        self.whole
    }

    pub fn lines(&self) -> Vec<&[u8]> {
        let Some(last) = self.bytes.iter().rposition(|&b| b == b'\n') else {
            return Vec::new();
        };
        let mut lines = self.bytes[..last]
            .split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>();
        if !self.whole && !lines.is_empty() {
            lines.remove(0);
        }
        lines
    }
}

pub fn widen<T>(path: &str, mut look: impl FnMut(&Window) -> (T, bool)) -> Option<T> {
    let mut found = None;
    for len in WINDOWS {
        let window = Window::read(path, len).ok()?;
        let (value, done) = look(&window);
        found = Some(value);
        if done || window.whole() {
            break;
        }
    }
    found
}

pub struct Peeked {
    pub context: Option<u64>,
    pub recent: Option<Conversation>,
}

pub type TailParser = fn(&[&[u8]]) -> Conversation;

pub fn peek(
    path: &str,
    held: fn(&[u8]) -> Option<u64>,
    recent: Option<TailParser>,
) -> Option<Peeked> {
    let mut peeked = Peeked {
        context: None,
        recent: None,
    };
    let mut stepped = recent.is_none();
    widen(path, |window| {
        let lines = window.lines();
        if peeked.context.is_none() {
            peeked.context = lines.iter().rev().find_map(|line| held(line));
        }
        if let (Some(parse), false) = (recent, stepped) {
            let conversation = parse(&lines);
            stepped = has_step(&conversation);
            peeked.recent = Some(conversation);
        }
        ((), peeked.context.is_some() && stepped)
    })?;
    Some(peeked)
}

pub fn has(line: &[u8], needle: &[u8]) -> bool {
    line.windows(needle.len()).any(|w| w == needle)
}

pub fn has_step(conversation: &Conversation) -> bool {
    conversation
        .items
        .iter()
        .any(|item| !matches!(item.kind, ItemKind::Notice { .. }))
}

#[cfg(test)]
#[path = "glance_tests.rs"]
mod tests;
