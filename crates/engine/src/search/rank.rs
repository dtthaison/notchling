use super::meta::SessionMeta;
use super::snippet::{self, find};

pub const RECENT_SECS: f64 = 14.0 * 86400.0;
pub const DEFAULT_LIMIT: usize = 30;

pub fn lowercase(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

pub fn words(query: &str) -> Vec<String> {
    lowercase(query)
        .split(char::is_whitespace)
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

#[derive(Clone, Debug)]
pub struct IndexedSession {
    pub meta: SessionMeta,
    pub text: String,
    pub(crate) title_lower: String,
    pub(crate) folder_lower: String,
    pub(crate) lower: String,
}

impl IndexedSession {
    pub fn new(meta: SessionMeta, text: String) -> Self {
        Self {
            title_lower: lowercase(&meta.title),
            folder_lower: lowercase(&meta.folder),
            lower: lowercase(&text),
            meta,
            text,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionHit {
    pub meta: SessionMeta,
    pub snippet: Option<String>,
    pub score: f64,
}

pub fn rank<W: AsRef<str>>(
    words: &[W],
    sessions: &[IndexedSession],
    now: f64,
    limit: usize,
) -> Vec<SessionHit> {
    let words: Vec<&str> = words
        .iter()
        .map(AsRef::as_ref)
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    let phrase = (words.len() > 1).then(|| words.join(" "));
    let mut scored: Vec<(&IndexedSession, f64)> = sessions
        .iter()
        .filter_map(|s| score(&words, phrase.as_deref(), s, now).map(|sc| (s, sc)))
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                b.0.meta
                    .modified
                    .partial_cmp(&a.0.meta.modified)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(s, score)| SessionHit {
            meta: s.meta.clone(),
            snippet: snippet::snippet(s, &words, snippet::DEFAULT_RADIUS),
            score,
        })
        .collect()
}

fn score(words: &[&str], phrase: Option<&str>, session: &IndexedSession, now: f64) -> Option<f64> {
    let mut score = 0.0;
    let mut in_title = 0;
    for word in words {
        let title = session.title_lower.contains(word);
        let folder = session.folder_lower.contains(word);
        let hits = count(word, &session.lower, 5);
        if !(title || folder || hits > 0) {
            return None;
        }
        if title {
            score += 30.0;
            in_title += 1;
        }
        if folder {
            score += 8.0;
        }
        if hits > 0 {
            score += 3.0 + hits as f64 * 2.0;
        }
    }
    if in_title == words.len() {
        score += 20.0;
    }
    if let Some(phrase) = phrase {
        if session.title_lower.contains(phrase) {
            score += 30.0;
        }
        if find(phrase, &session.lower, 0).is_some() {
            score += 12.0;
        }
    }
    let age = (now - session.meta.modified).max(0.0);
    score += 10.0 * (1.0 - age / RECENT_SECS).max(0.0);
    Some(score)
}

fn count(needle: &str, haystack: &str, cap: usize) -> usize {
    let (mut n, mut at) = (0, 0);
    while n < cap {
        match find(needle, haystack, at) {
            Some(hit) => {
                n += 1;
                at = hit + needle.len().max(1);
            }
            None => break,
        }
    }
    n
}

pub fn latest(sessions: &[IndexedSession], limit: usize) -> Vec<SessionHit> {
    let mut sorted: Vec<&IndexedSession> = sessions.iter().collect();
    sorted.sort_by(|a, b| {
        b.meta
            .modified
            .partial_cmp(&a.meta.modified)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted
        .into_iter()
        .take(limit)
        .map(|s| SessionHit {
            meta: s.meta.clone(),
            snippet: None,
            score: 0.0,
        })
        .collect()
}
