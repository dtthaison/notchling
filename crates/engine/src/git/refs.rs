use serde::Serialize;
use std::collections::HashMap;

pub const REF_FORMAT: &str = "%(refname)%00%(objectname)%00%(committerdate:unix)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(subject)";
pub const MAX_REFS: usize = 300;
pub const SUBJECT_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub ahead: u64,
    pub behind: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Track {
    pub ahead: u64,
    pub behind: u64,
    pub gone: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Lines {
    pub added: u64,
    pub removed: u64,
    pub files: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RefRow {
    pub refname: String,
    pub commit: String,
    pub committed_ms: Option<i64>,
    pub upstream: Option<String>,
    pub track: Track,
    pub subject: String,
}

fn is_remote_head(refname: &str) -> bool {
    refname
        .strip_prefix("refs/remotes/")
        .is_some_and(|rest| rest.ends_with("/HEAD"))
}

fn non_empty(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_string())
}

fn parse_ref(line: &str) -> Option<RefRow> {
    let mut fields = line.splitn(6, '\0');
    let refname = fields.next().filter(|name| !name.is_empty())?;
    let commit = fields.next()?;
    let committed = fields.next()?;
    let upstream = fields.next()?;
    let track = fields.next()?;
    let subject = fields.next()?;
    Some(RefRow {
        refname: refname.to_string(),
        commit: commit.to_string(),
        committed_ms: committed
            .parse::<i64>()
            .ok()
            .and_then(|seconds| seconds.checked_mul(1000)),
        upstream: non_empty(upstream),
        track: parse_track(track),
        subject: plain_label(subject, SUBJECT_CHARS),
    })
}

pub fn parse_refs(text: &str) -> (Vec<RefRow>, bool) {
    let mut rows = Vec::new();
    for line in text.lines() {
        let Some(row) = parse_ref(line) else {
            continue;
        };
        if is_remote_head(&row.refname) {
            continue;
        }
        if rows.len() == MAX_REFS {
            return (rows, true);
        }
        rows.push(row);
    }
    (rows, false)
}

fn leading_number(part: &str, word: &str) -> Option<u64> {
    part.trim().strip_prefix(word)?.trim().parse::<u64>().ok()
}

pub fn parse_track(text: &str) -> Track {
    let text = text.trim();
    if text == "gone" {
        return Track {
            gone: true,
            ..Track::default()
        };
    }
    let mut track = Track::default();
    for part in text.split(',') {
        if let Some(ahead) = leading_number(part, "ahead") {
            track.ahead = ahead;
        } else if let Some(behind) = leading_number(part, "behind") {
            track.behind = behind;
        }
    }
    track
}

fn two_numbers(text: &str) -> Option<(u64, u64)> {
    let mut words = text.split_whitespace();
    let first = words.next()?.parse::<u64>().ok()?;
    let second = words.next()?.parse::<u64>().ok()?;
    words.next().is_none().then_some((first, second))
}

pub fn parse_ahead_behind(text: &str) -> HashMap<String, Counts> {
    let mut counts = HashMap::new();
    for line in text.lines() {
        let Some((refname, numbers)) = line.split_once('\0') else {
            continue;
        };
        if let Some((ahead, behind)) = two_numbers(numbers) {
            counts.insert(refname.to_string(), Counts { ahead, behind });
        }
    }
    counts
}

pub fn parse_left_right(text: &str) -> Option<Counts> {
    let (behind, ahead) = two_numbers(text)?;
    Some(Counts { ahead, behind })
}

pub fn parse_shortstat(text: &str) -> Lines {
    let mut lines = Lines::default();
    for part in text.split(',') {
        let mut words = part.split_whitespace();
        let Some(number) = words.next().and_then(|word| word.parse::<u64>().ok()) else {
            continue;
        };
        match words.next() {
            Some(word) if word.starts_with("file") => lines.files = number,
            Some(word) if word.starts_with("insertion") => lines.added = number,
            Some(word) if word.starts_with("deletion") => lines.removed = number,
            _ => {}
        }
    }
    lines
}

fn host_chars_ok(host: &str) -> bool {
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']'))
}

fn host_of_authority(authority: &str) -> Option<String> {
    let hostport = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(inner) = hostport.strip_prefix('[') {
        let end = inner.find(']')?;
        &hostport[..end + 2]
    } else {
        hostport.split(':').next().unwrap_or("")
    };
    host_chars_ok(host).then(|| host.to_ascii_lowercase())
}

fn host_of_url(url: &str) -> Option<String> {
    let url = url.trim();
    if let Some((_, rest)) = url.split_once("://") {
        let authority = rest.split('/').next().unwrap_or("");
        return host_of_authority(authority);
    }
    let (before, _) = url.split_once(':')?;
    if before.contains('/') {
        return None;
    }
    let host = before.rsplit('@').next().unwrap_or(before);
    host_chars_ok(host).then(|| host.to_ascii_lowercase())
}

pub fn remote_hosts(config_z: &str) -> Vec<(String, Option<String>)> {
    let mut remotes = Vec::new();
    for record in config_z.split('\0') {
        let Some((key, url)) = record.split_once('\n') else {
            continue;
        };
        let Some(name) = key
            .strip_prefix("remote.")
            .and_then(|rest| rest.strip_suffix(".url"))
        else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        remotes.push((name.to_string(), host_of_url(url)));
    }
    remotes
}

fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

pub fn plain_label(text: &str, max_chars: usize) -> String {
    let kept: String = text
        .chars()
        .filter(|c| !c.is_control() && !is_bidi_control(*c))
        .take(max_chars)
        .collect();
    kept.trim().to_string()
}
