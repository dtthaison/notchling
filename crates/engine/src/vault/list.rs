use super::entry::{valid_name, valid_var, Kind, VaultEntry, VaultValue};
use super::expiry;
use super::url;
use crate::docfile::{DocFile, UNREADABLE};
use crate::organizations;
use crate::store::{self, DocChange};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexError {
    Unreadable,
    Write(String),
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexError::Unreadable => {
                f.write_str("The vault list (vault.json) can't be read, so it was left as it is.")
            }
            IndexError::Write(why) => write!(f, "The vault list couldn't be saved: {why}"),
        }
    }
}

fn sorted(mut list: Vec<VaultEntry>) -> Vec<VaultEntry> {
    list.sort_by_key(VaultEntry::id);
    list
}

pub fn decode(data: &[u8]) -> Vec<VaultEntry> {
    sorted(serde_json::from_slice(data).unwrap_or_default())
}

pub fn encode(list: &[VaultEntry]) -> Vec<u8> {
    let list = sorted(list.to_vec());
    serde_json::to_vec_pretty(&list).unwrap_or_else(|_| b"[]".to_vec())
}

pub const INDEX_DOC: &str = "daemon/vault";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShadowEdit {
    Move { from: String, to: String },
    Remove(String),
}

impl ShadowEdit {
    fn apply(&self, shadow: &mut std::collections::BTreeMap<String, String>) -> bool {
        match self {
            ShadowEdit::Move { from, to } => match shadow.remove(from) {
                Some(value) => {
                    shadow.insert(to.clone(), value);
                    true
                }
                None => false,
            },
            ShadowEdit::Remove(key) => shadow.remove(key).is_some(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultIndex(DocFile);

impl VaultIndex {
    pub fn new(data_dir: &Path, old_file: PathBuf) -> VaultIndex {
        VaultIndex(DocFile::new(data_dir, INDEX_DOC, old_file).strict())
    }

    pub fn data_dir(&self) -> &Path {
        &self.0.data_dir
    }

    pub fn in_data(data_dir: &Path) -> VaultIndex {
        VaultIndex::new(data_dir, data_dir.join("daemon").join("vault.json"))
    }

    pub fn read(&self) -> Result<Vec<VaultEntry>, IndexError> {
        match self.0.read() {
            Ok(None) => Ok(Vec::new()),
            Ok(Some(text)) => serde_json::from_str(&text)
                .map(sorted)
                .map_err(|_| IndexError::Unreadable),
            Err(_) => Err(IndexError::Unreadable),
        }
    }

    pub fn update(
        &self,
        change: impl FnOnce(Vec<VaultEntry>) -> Result<Vec<VaultEntry>, IndexError>,
    ) -> Result<(), IndexError> {
        self.update_shadow(change, None)
    }

    pub fn update_shadow(
        &self,
        change: impl FnOnce(Vec<VaultEntry>) -> Result<Vec<VaultEntry>, IndexError>,
        shadow: Option<ShadowEdit>,
    ) -> Result<(), IndexError> {
        let next = |row: Option<String>| -> Result<String, String> {
            let list = match row {
                None => Vec::new(),
                Some(text) => serde_json::from_str(&text)
                    .map(sorted)
                    .map_err(|_| UNREADABLE.to_owned())?,
            };
            let list = change(list).map_err(|e| match e {
                IndexError::Unreadable => UNREADABLE.to_owned(),
                IndexError::Write(why) => why,
            })?;
            String::from_utf8(encode(&list)).map_err(|e| e.to_string())
        };
        let result = match shadow {
            None => self
                .0
                .update(is_list, |row| next(row).map(Some))
                .map(|_| ()),
            Some(edit) => {
                self.0.import(is_list);
                store::update_docs(&self.0.data_dir, &[INDEX_DOC, organizations::DOC], |old| {
                    if self.0.strict
                        && old[0].is_none()
                        && std::fs::symlink_metadata(&self.0.old_file).is_ok()
                    {
                        return Err(UNREADABLE.to_owned());
                    }
                    let text = next(old[0].clone())?;
                    let mut changes = vec![DocChange::Set(text), DocChange::Keep];
                    if let Some(doc_text) = old[1].as_deref() {
                        if let Ok(mut doc) = organizations::Document::parse(doc_text) {
                            if edit.apply(&mut doc.shadow.vault) {
                                changes[1] = DocChange::Set(doc.text()?);
                            }
                        }
                    }
                    Ok(changes)
                })
            }
        };
        result.map_err(|why| {
            if why == UNREADABLE {
                IndexError::Unreadable
            } else {
                IndexError::Write(why)
            }
        })
    }

    pub fn write(&self, list: &[VaultEntry]) -> Result<(), IndexError> {
        self.update(|_| Ok(list.to_vec()))
    }

    pub fn stamp(&self) -> Option<(std::time::SystemTime, u64)> {
        use std::hash::{Hash, Hasher};
        let text = self.0.read().ok()??;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_nanos(hasher.finish());
        Some((at, text.len() as u64))
    }
}

fn is_list(text: &str) -> bool {
    serde_json::from_str::<Vec<VaultEntry>>(text).is_ok()
}

pub fn problem(
    entry: &VaultEntry,
    list: &[VaultEntry],
    old: Option<&VaultEntry>,
) -> Option<String> {
    if !valid_name(&entry.name) {
        return Some("Use letters, digits, - _ or . for the name, starting with a letter.".into());
    }
    if let Some(bad) = entry.variables().into_iter().find(|v| !valid_var(v)) {
        return Some(format!("\"{bad}\" isn't a valid variable name."));
    }
    let all = entry.all_variables();
    if all.iter().collect::<BTreeSet<_>>().len() != all.len() {
        return Some(format!(
            "Each value needs its own variable ({} is the URL's).",
            entry.url_var().unwrap_or_else(|| "none".into())
        ));
    }
    let old_id = old.map(VaultEntry::id);
    let others: Vec<&VaultEntry> = list.iter().filter(|e| Some(e.id()) != old_id).collect();
    if others.iter().any(|e| e.id() == entry.id()) {
        return Some(format!("There's already a secret named {}.", entry.name));
    }
    let taken: BTreeSet<String> = others.iter().flat_map(|e| e.all_variables()).collect();
    if let Some(clash) = all.iter().find(|v| taken.contains(*v)) {
        return Some(format!("Another secret already uses {clash}."));
    }
    None
}

pub fn find(query: &str, list: &[VaultEntry]) -> Vec<VaultEntry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return list.to_vec();
    }
    let q_host = url::host_of(&q);
    let words: Vec<&str> = q
        .split(|c: char| !(c.is_alphanumeric() || matches!(c, '.' | '-' | '_')))
        .filter(|w| w.chars().count() >= 2)
        .collect();
    let last_two = |h: &str| {
        let parts: Vec<&str> = h.split('.').collect();
        parts[parts.len().saturating_sub(2)..].join(".")
    };
    let score = |e: &VaultEntry| {
        let mut s = 0;
        if let (Some(qh), Some(h)) = (&q_host, e.host()) {
            if &h == qh {
                s += 10;
            } else if qh.ends_with(&format!(".{h}")) || h.ends_with(&format!(".{qh}")) {
                s += 6;
            } else if last_two(&h) == last_two(qh) {
                s += 3;
            }
        }
        if e.id() == q {
            s += 8;
        }
        let text = format!(
            "{} {} {} {}",
            e.name,
            e.url.as_deref().unwrap_or(""),
            e.note,
            e.how_to.as_deref().unwrap_or("")
        )
        .to_lowercase();
        s + words.iter().filter(|w| text.contains(*w)).count()
    };
    let mut scored: Vec<(usize, &VaultEntry)> = list
        .iter()
        .map(|e| (score(e), e))
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by_key(|s| std::cmp::Reverse(s.0));
    scored.into_iter().map(|(_, e)| e.clone()).collect()
}

pub fn environment(
    entries: &[(VaultEntry, VaultValue)],
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let mut env = BTreeMap::new();
    let mut hide = BTreeMap::new();
    for (entry, value) in entries {
        env.insert(entry.secret_var.clone(), value.secret.clone());
        if let (Some(var), Some(u)) = (entry.url_var(), &entry.url) {
            env.insert(var, u.clone());
        }
        hide.insert(value.secret.clone(), format!("[secret:{}]", entry.name));
        if entry.kind == Kind::Login {
            env.insert(entry.user_var.clone(), value.username.clone());
            if !value.username.is_empty() && !hide.contains_key(&value.username) {
                hide.insert(
                    value.username.clone(),
                    format!("[secret:{} username]", entry.name),
                );
            }
        }
    }
    (env, hide)
}

pub fn label(e: &VaultEntry) -> String {
    match e.host() {
        Some(host) => format!("{} ({host})", e.name),
        None => e.name.clone(),
    }
}

pub fn describe(e: &VaultEntry, today: i64) -> String {
    let vars: Vec<String> = e.all_variables().iter().map(|v| format!("${v}")).collect();
    let mut lines = vec![format!(
        "{}  ({})  {}",
        e.name,
        e.kind.as_str(),
        vars.join(" ")
    )];
    if let Some(u) = e.url.as_deref().filter(|u| !u.is_empty()) {
        lines.push(format!("    {u}"));
    }
    lines.extend(indented(&e.note, 4));
    if let Some(how) = e.how_to.as_deref().filter(|h| !h.trim().is_empty()) {
        lines.push("    How to get/renew:".into());
        lines.extend(indented(how, 6));
    }
    if let Some(day) = &e.expires {
        lines.push(format!("    {}", expiry::line(day, today)));
    }
    if e.asks() {
        lines.push("    The user approves each use.".into());
    }
    lines.join("\n")
}

fn indented(text: &str, spaces: usize) -> Vec<String> {
    let pad = " ".repeat(spaces);
    let mut lines: Vec<&str> = text.split(['\n', '\r']).collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                format!("{pad}{l}")
            }
        })
        .collect()
}
