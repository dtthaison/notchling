use super::{HEAD_BYTES, TAIL_BYTES, WHOLE_FILE_BYTES};
use crate::search::{
    claude_line, codex_line, IndexedSession, SessionMeta, SessionTextBuilder, SessionTool,
    MAX_LINE_BYTES,
};
use crate::transcript::codex as codex_transcript;
use crate::transcript::scan::{scan_lines, Line};
use crate::transcript::tail::{tail_start_with, LONG_LINE_SCAN};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub(super) enum Read1 {
    Gone,
    Empty,
    Built(Box<IndexedSession>),
}

pub(super) fn contents(path: &str, from: u64) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let from = from.min(size);
    file.seek(SeekFrom::Start(from)).ok()?;
    if size - from <= WHOLE_FILE_BYTES {
        let mut data = Vec::new();
        (&mut file)
            .take(WHOLE_FILE_BYTES)
            .read_to_end(&mut data)
            .ok()?;
        return Some(data);
    }
    let mut head = Vec::new();
    (&mut file).take(HEAD_BYTES).read_to_end(&mut head).ok()?;
    if let Some(cut) = head.iter().rposition(|&b| b == b'\n') {
        head.truncate(cut + 1);
    } else {
        head.clear();
    }
    let start = tail_start_with(path, size, TAIL_BYTES, LONG_LINE_SCAN, MAX_LINE_BYTES)
        .ok()?
        .max(from);
    head.truncate(usize::try_from(start - from).unwrap_or(usize::MAX));
    scan_lines(path, start, size, false, MAX_LINE_BYTES, |line| {
        if let Line::Whole(line) = line {
            head.extend_from_slice(line);
            head.push(b'\n');
        }
    })
    .ok()?;
    Some(head)
}

fn lines(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    data.split(|&b| b == b'\n')
        .filter(|l| !l.is_empty() && l.len() <= MAX_LINE_BYTES)
}

pub(super) fn read(candidate: &SessionMeta, tool: SessionTool) -> Read1 {
    let from = match tool {
        SessionTool::Codex => codex_transcript::fork::own_start(&candidate.path),
        _ => 0,
    };
    let Some(data) = contents(&candidate.path, from) else {
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
            if let Some(lineage) = codex_transcript::lineage(&candidate.path) {
                if lineage.parent.is_empty() {
                    return Read1::Empty;
                }
                meta.parent = Some(format!("codex:{}", lineage.parent));
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::testutil::TempDir;

    fn lines_of(data: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(data)
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn pictures_do_not_push_the_text_between_them_out_of_a_big_file() {
        let dir = TempDir::new("idx-pictures");
        let picture = "A".repeat(2_000_000);
        let mut text = String::new();
        for (n, part) in ["head", "mid", "late", "tail"].iter().enumerate() {
            for i in 0..3 {
                text.push_str(&format!("{part}-{i}\n"));
            }
            if n < 3 {
                text.push_str(&picture);
                text.push('\n');
            }
        }
        let path = dir.write("a.jsonl", &text);
        assert!(text.len() as u64 > WHOLE_FILE_BYTES);
        let data = contents(&path.to_string_lossy(), 0).expect("contents");
        let want: Vec<String> = ["head", "mid", "late", "tail"]
            .iter()
            .flat_map(|p| (0..3).map(move |i| format!("{p}-{i}")))
            .collect();
        assert_eq!(lines_of(&data), want, "every text line, no picture");
    }

    #[test]
    fn a_big_file_without_pictures_is_its_start_and_its_end() {
        let dir = TempDir::new("idx-plain");
        let line = "x".repeat(99);
        let count = 90_000;
        let text: String = (0..count).map(|i| format!("{i:05}{line}\n")).collect();
        let path = dir.write("a.jsonl", &text);
        assert!(text.len() as u64 > WHOLE_FILE_BYTES + TAIL_BYTES);
        let data = contents(&path.to_string_lossy(), 0).expect("contents");
        assert!(data.len() as u64 <= HEAD_BYTES + TAIL_BYTES + 1_000);
        let lines = lines_of(&data);
        assert!(lines[0].starts_with("00000"));
        assert!(lines[lines.len() - 1].starts_with(&format!("{:05}", count - 1)));
        assert!(
            lines.len() < count,
            "the middle of a file this big is left out"
        );
    }
}
