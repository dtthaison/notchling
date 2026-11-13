use super::rank::lowercase;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawEntry")]
pub struct PageEntry {
    pub folder: String,
    pub file: String,
    pub title: String,
    pub url: String,
    pub source: String,
    pub project: String,
    pub account: String,
    pub updated: f64,
    pub version: String,
}

#[derive(Deserialize)]
struct RawEntry {
    folder: String,
    file: String,
    title: Option<String>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    project: String,
    #[serde(default)]
    account: String,
    #[serde(default)]
    updated: f64,
    #[serde(default)]
    version: String,
}

impl From<RawEntry> for PageEntry {
    fn from(r: RawEntry) -> Self {
        Self {
            title: r.title.unwrap_or_else(|| r.folder.clone()),
            folder: r.folder,
            file: r.file,
            url: r.url,
            source: r.source,
            project: r.project,
            account: r.account,
            updated: r.updated,
            version: r.version,
        }
    }
}

impl PageEntry {
    pub fn artifact_id(&self) -> Option<&str> {
        if self.url.is_empty() {
            None
        } else {
            self.url.split('/').next_back()
        }
    }

    pub fn matches<W: AsRef<str>>(&self, words: &[W]) -> bool {
        let text = lowercase(&format!(
            "{} {} {} {}",
            self.title, self.project, self.account, self.url
        ));
        words.iter().all(|w| text.contains(w.as_ref()))
    }
}

fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

pub fn artifact_id_in(text: &str) -> Option<String> {
    let t = text.trim();
    let mut from = 0;
    while let Some(i) = t[from..].find("claude.ai/") {
        let rest = &t[from + i + "claude.ai/".len()..];
        let rest = rest
            .strip_prefix("code/")
            .filter(|r| r.starts_with("artifact/"))
            .unwrap_or(rest);
        if let Some(after) = rest.strip_prefix("artifact/") {
            let id: String = after.chars().take_while(|&c| is_id_char(c)).collect();
            if !id.is_empty() {
                return Some(id);
            }
        }
        from += i + 1;
    }
    (t.chars().count() >= 16 && t.chars().all(is_id_char)).then(|| t.to_string())
}

pub fn find_pages<'a>(query: &str, all: &'a [PageEntry]) -> Vec<&'a PageEntry> {
    if let Some(id) = artifact_id_in(query) {
        let exact: Vec<_> = all
            .iter()
            .filter(|e| e.artifact_id() == Some(id.as_str()))
            .collect();
        if !exact.is_empty() {
            return exact;
        }
        let id = lowercase(&id);
        let loose: Vec<_> = all
            .iter()
            .filter(|e| e.artifact_id().map(lowercase).as_deref() == Some(id.as_str()))
            .collect();
        if !loose.is_empty() {
            return loose;
        }
    }
    let lowered = lowercase(query);
    let words: Vec<&str> = lowered.split(' ').filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return all.iter().collect();
    }
    all.iter().filter(|e| e.matches(&words)).collect()
}
