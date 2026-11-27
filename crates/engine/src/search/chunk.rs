use super::{SessionRole, TranscriptLine};
use sha2::{Digest, Sha256};

pub const QUIET_SECS: f64 = 60.0;

const HEAD_CHARS: usize = 600;
const WINDOW_CHARS: usize = 1200;
const OVERLAP_CHARS: usize = 150;
const PROMPT_CHARS: usize = 200;
const CODE_LINES: usize = 20;
const WINDOWS_PER_TURN: usize = 6;
const CHUNKS_PER_SESSION: usize = 200;
const CHUNKS_PER_PAGE: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub prompt: String,
    pub reply: String,
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkKind {
    Head,
    Turn,
    Page,
    Entry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkText {
    pub kind: ChunkKind,
    pub text: String,
}

pub fn hash(text: &str) -> [u8; 16] {
    let digest = Sha256::digest(text.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out
}

pub fn turns(lines: impl IntoIterator<Item = TranscriptLine>, quiet: bool) -> Vec<Turn> {
    let mut out: Vec<Turn> = Vec::new();
    for line in lines {
        let TranscriptLine::Message { role, text, .. } = line else {
            continue;
        };
        match role {
            SessionRole::User => out.push(Turn {
                prompt: text,
                reply: String::new(),
                closed: true,
            }),
            SessionRole::Assistant => {
                if let Some(turn) = out.last_mut() {
                    if !turn.reply.is_empty() {
                        turn.reply.push_str("\n\n");
                    }
                    turn.reply.push_str(&text);
                }
            }
        }
    }
    if let Some(last) = out.last_mut() {
        last.closed = quiet;
    }
    out
}

fn take_chars(text: &str, n: usize) -> &str {
    match text.char_indices().nth(n) {
        Some((i, _)) => &text[..i],
        None => text,
    }
}

fn tail_chars(text: &str, n: usize) -> &str {
    let total = text.chars().count();
    if total <= n {
        return text;
    }
    match text.char_indices().nth(total - n) {
        Some((i, _)) => &text[i..],
        None => text,
    }
}

fn last_part(folder: &str) -> &str {
    folder
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
}

fn trim_code_blocks(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut in_fence = false;
    let mut kept = 0;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            kept = 0;
            out.push(line);
        } else if in_fence {
            if kept < CODE_LINES {
                out.push(line);
            }
            kept += 1;
        } else {
            out.push(line);
        }
    }
    out.join("\n")
}

fn sentences(paragraph: &str, limit: usize) -> Vec<String> {
    let chars: Vec<char> = paragraph.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let end = matches!(c, '.' | '!' | '?' | '\u{3002}' | '\u{2026}')
            && chars.get(i + 1).is_none_or(|n| n.is_whitespace());
        i += 1;
        if end || c == '\n' {
            while i < chars.len() && chars[i].is_whitespace() && chars[i] != '\n' {
                i += 1;
            }
            out.push(chars[start..i].iter().collect::<String>());
            start = i;
        }
    }
    if start < chars.len() {
        out.push(chars[start..].iter().collect());
    }
    let mut packed: Vec<String> = Vec::new();
    for sentence in out {
        let len = sentence.chars().count();
        if len > limit {
            let all: Vec<char> = sentence.chars().collect();
            for part in all.chunks(limit) {
                packed.push(part.iter().collect());
            }
            continue;
        }
        match packed.last_mut() {
            Some(last) if last.chars().count() + len <= limit => last.push_str(&sentence),
            _ => packed.push(sentence),
        }
    }
    packed
}

fn atoms(text: &str, limit: usize) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    for paragraph in text.split("\n\n") {
        let paragraph = paragraph.trim_matches('\n');
        if paragraph.trim().is_empty() {
            continue;
        }
        if paragraph.chars().count() <= limit {
            out.push(("\n\n", paragraph.to_string()));
            continue;
        }
        for (n, piece) in sentences(paragraph, limit).into_iter().enumerate() {
            out.push((if n == 0 { "\n\n" } else { "" }, piece));
        }
    }
    out
}

fn windows(text: &str, budget: usize, max: usize) -> Vec<String> {
    let limit = budget.saturating_sub(OVERLAP_CHARS + 2).max(1);
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0;
    let mut fresh = false;
    for (sep, atom) in atoms(text, limit) {
        let sep_len = if cur.is_empty() {
            0
        } else {
            sep.chars().count()
        };
        let add = sep_len + atom.chars().count();
        if fresh && cur_len + add > budget {
            out.push(cur.trim().to_string());
            if out.len() >= max {
                return out;
            }
            cur = tail_chars(cur.trim_end(), OVERLAP_CHARS).to_string();
            cur_len = cur.chars().count();
        }
        if !cur.is_empty() {
            cur.push_str(sep);
            cur_len += sep.chars().count();
        }
        cur.push_str(&atom);
        cur_len += atom.chars().count();
        fresh = true;
    }
    if fresh && out.len() < max {
        out.push(cur.trim().to_string());
    }
    out.retain(|w| !w.is_empty());
    out
}

fn head_chunk(title: &str, folder: &str, turns: &[Turn]) -> Option<ChunkText> {
    let mut parts: Vec<&str> = Vec::new();
    for part in [
        title.trim(),
        last_part(folder).trim(),
        turns.first().map_or("", |t| t.prompt.trim()),
    ] {
        if !part.is_empty() {
            parts.push(part);
        }
    }
    if parts.is_empty() {
        return None;
    }
    let joined = parts.join("\n");
    Some(ChunkText {
        kind: ChunkKind::Head,
        text: take_chars(&joined, HEAD_CHARS).trim().to_string(),
    })
}

fn turn_chunks(turn: &Turn) -> Vec<ChunkText> {
    let prompt = take_chars(turn.prompt.trim(), PROMPT_CHARS).trim_end();
    let prefix = if prompt.is_empty() {
        String::new()
    } else {
        format!("{prompt}\n\n")
    };
    let budget = WINDOW_CHARS - prefix.chars().count();
    let reply = trim_code_blocks(&turn.reply);
    let bodies = windows(&reply, budget, WINDOWS_PER_TURN);
    if bodies.is_empty() {
        if prompt.is_empty() {
            return Vec::new();
        }
        return vec![ChunkText {
            kind: ChunkKind::Turn,
            text: prompt.to_string(),
        }];
    }
    bodies
        .into_iter()
        .map(|body| ChunkText {
            kind: ChunkKind::Turn,
            text: format!("{prefix}{body}"),
        })
        .collect()
}

pub fn session_chunks(title: &str, folder: &str, turns: &[Turn]) -> Vec<ChunkText> {
    let mut out: Vec<ChunkText> = head_chunk(title, folder, turns).into_iter().collect();
    for turn in turns.iter().rev().filter(|t| t.closed) {
        if out.len() >= CHUNKS_PER_SESSION {
            break;
        }
        for chunk in turn_chunks(turn) {
            if out.len() >= CHUNKS_PER_SESSION {
                break;
            }
            out.push(chunk);
        }
    }
    out
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            let num = name.strip_prefix('#')?;
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => num.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest[1..]
            .find(';')
            .filter(|&n| n <= 8)
            .and_then(|n| entity(&rest[1..1 + n]).map(|c| (c, n)));
        match decoded {
            Some((c, n)) => {
                out.push(c);
                rest = &rest[n + 2..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn tidy(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() && lines.last().is_none_or(String::is_empty) {
            continue;
        }
        lines.push(line);
    }
    lines.join("\n").trim().to_string()
}

fn tag_name(tag: &str) -> (String, bool) {
    let inner = tag.trim_start_matches('<');
    let closing = inner.starts_with('/');
    let name: String = inner
        .trim_start_matches('/')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    (name, closing)
}

fn push_text(cur: &mut String, text: &str) {
    cur.extend(
        text.chars()
            .map(|c| if c.is_whitespace() { ' ' } else { c }),
    );
}

fn page_sections(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut sections: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < html.len() {
        let Some(off) = html[i..].find('<') else {
            push_text(&mut cur, &html[i..]);
            break;
        };
        push_text(&mut cur, &html[i..i + off]);
        i += off;
        if html[i..].starts_with("<!--") {
            i = html[i..].find("-->").map_or(html.len(), |n| i + n + 3);
            continue;
        }
        let Some(end) = html[i..].find('>') else {
            break;
        };
        let (name, closing) = tag_name(&html[i..i + end]);
        let after = i + end + 1;
        if name.is_empty() {
            cur.push('<');
            i += 1;
            continue;
        }
        i = after;
        if !closing && matches!(name.as_str(), "script" | "style" | "noscript" | "template") {
            let close = format!("</{name}");
            i = lower[i..].find(&close).map_or(html.len(), |n| {
                let at = i + n;
                html[at..].find('>').map_or(html.len(), |m| at + m + 1)
            });
            continue;
        }
        if !closing && matches!(name.as_str(), "h1" | "h2" | "h3") {
            sections.push(std::mem::take(&mut cur));
        }
        match name.as_str() {
            "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "aside" | "ul"
            | "ol" | "table" | "pre" | "blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                cur.push_str("\n\n")
            }
            "br" | "li" | "tr" | "dt" | "dd" => cur.push('\n'),
            _ => {}
        }
    }
    sections.push(cur);
    sections
}

pub fn page_chunks(html: &str) -> Vec<ChunkText> {
    let mut out = Vec::new();
    for section in page_sections(html) {
        let text = tidy(&decode(&section));
        if text.is_empty() {
            continue;
        }
        for body in windows(&text, WINDOW_CHARS, usize::MAX) {
            if out.len() >= CHUNKS_PER_PAGE {
                return out;
            }
            out.push(ChunkText {
                kind: ChunkKind::Page,
                text: body,
            });
        }
    }
    out
}
