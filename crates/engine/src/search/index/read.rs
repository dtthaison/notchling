use super::{HEAD_BYTES, TAIL_BYTES, WHOLE_FILE_BYTES};
use crate::search::{
    claude_line, codex_line, IndexedSession, SessionMeta, SessionTextBuilder, SessionTool,
    MAX_LINE_BYTES,
};
use crate::transcript::codex as codex_transcript;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub(super) enum Read1 {
    Gone,
    Empty,
    Built(Box<IndexedSession>),
}

pub(super) fn contents(path: &str) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    if size <= WHOLE_FILE_BYTES {
        let mut data = Vec::new();
        (&mut file)
            .take(WHOLE_FILE_BYTES)
            .read_to_end(&mut data)
            .ok()?;
        return Some(data);
    }
    let mut head = Vec::new();
    (&mut file).take(HEAD_BYTES).read_to_end(&mut head).ok()?;
    file.seek(SeekFrom::Start(size.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut tail = Vec::new();
    (&mut file).take(TAIL_BYTES).read_to_end(&mut tail).ok()?;
    if let Some(cut) = head.iter().rposition(|&b| b == b'\n') {
        head.truncate(cut + 1);
    } else {
        head.clear();
    }
    if let Some(cut) = tail.iter().position(|&b| b == b'\n') {
        tail.drain(..=cut);
    } else {
        tail.clear();
    }
    head.extend_from_slice(&tail);
    Some(head)
}

fn lines(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    data.split(|&b| b == b'\n')
        .filter(|l| !l.is_empty() && l.len() <= MAX_LINE_BYTES)
}

pub(super) fn read(candidate: &SessionMeta, tool: SessionTool) -> Read1 {
    let Some(data) = contents(&candidate.path) else {
        return Read1::Gone;
    };
    let mut builder = SessionTextBuilder::new();
    let mut meta = candidate.clone();
    let mut codex_meta = None;
    match tool {
        SessionTool::Claude => {
            for line in lines(&data) {
                if let Some(l) = claude_line(line) {
                    builder.add(l);
                }
            }
        }
        SessionTool::Codex => {
            let Some(info) = codex_transcript::meta(&candidate.path) else {
                return Read1::Empty;
            };
            meta.folder.clone_from(&info.cwd);
            codex_meta = Some(info);
            for line in lines(&data) {
                if let Some(l) = codex_line(line) {
                    builder.add(l);
                }
            }
        }
        SessionTool::Antigravity => return Read1::Empty,
    }
    let text = builder.text();
    if text.is_empty() {
        return Read1::Empty;
    }
    if tool == SessionTool::Claude {
        meta.folder = builder.cwd().unwrap_or("").to_string();
    }
    meta.title = match codex_meta.and_then(|m| m.title) {
        Some(t) => t,
        None => builder.title(),
    };
    Read1::Built(Box::new(IndexedSession::new(meta, text)))
}
