//! The vault's list (Swift `SecretVault`'s `vault.json`): names, kinds, URLs, variables and notes,
//! never values. Reading and writing it ([`VaultIndex`]), checking an entry before a save, ranking
//! entries for a task, the environment a command gets, and the text `secrets` prints.

use super::entry::{valid_name, valid_var, Kind, VaultEntry, VaultValue};
use super::expiry;
use super::url;
use crate::docfile::{DocFile, UNREADABLE};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Why the list couldn't be read or written. Never holds a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexError {
    /// The list (the document, or the old `vault.json` not moved in yet) exists and isn't a list:
    /// a write-back would wipe it, so nothing is written.
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

/// The list a file holds, sorted by id; empty for anything that isn't one.
pub fn decode(data: &[u8]) -> Vec<VaultEntry> {
    sorted(serde_json::from_slice(data).unwrap_or_default())
}

/// The list as `vault.json` holds it: sorted, pretty, keys sorted.
pub fn encode(list: &[VaultEntry]) -> Vec<u8> {
    let list = sorted(list.to_vec());
    serde_json::to_vec_pretty(&list).unwrap_or_else(|_| b"[]".to_vec())
}

/// The document that holds the list in `<data>/notchling.db`.
pub const INDEX_DOC: &str = "daemon/vault";

/// Where the list is kept: the [`INDEX_DOC`] document in `<data>/notchling.db` (decision 0014; the
/// list was the file `<data>/daemon/vault.json`, which moves in once, on the first change, and is
/// read until then). No value is in it. A reader makes no database and moves nothing; a change
/// is one transaction, so the daemon and the Machines tool never lose each other's save. An old
/// file that can't be read, or isn't a list, stays where it is and nothing is written
/// ([`IndexError::Unreadable`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultIndex(DocFile);

impl VaultIndex {
    /// The list of `data_dir`, with `old_file` as the file it was.
    pub fn new(data_dir: &Path, old_file: PathBuf) -> VaultIndex {
        VaultIndex(DocFile::new(data_dir, INDEX_DOC, old_file).strict())
    }

    /// The daemon's own: `<data_dir>/daemon/vault.json` as the old file, never the Swift app's
    /// `<data_dir>/vault.json`.
    pub fn in_data(data_dir: &Path) -> VaultIndex {
        VaultIndex::new(data_dir, data_dir.join("daemon").join("vault.json"))
    }

    /// The list, sorted. Empty only when there is none yet; one that exists but can't be read or
    /// isn't a list is an error.
    pub fn read(&self) -> Result<Vec<VaultEntry>, IndexError> {
        match self.0.read() {
            Ok(None) => Ok(Vec::new()),
            Ok(Some(text)) => serde_json::from_str(&text)
                .map(sorted)
                .map_err(|_| IndexError::Unreadable),
            Err(_) => Err(IndexError::Unreadable),
        }
    }

    /// Reads the list and writes what `change` makes of it, in one transaction (sorted, pretty,
    /// keys sorted). An error from `change` writes nothing.
    pub fn update(
        &self,
        change: impl FnOnce(Vec<VaultEntry>) -> Result<Vec<VaultEntry>, IndexError>,
    ) -> Result<(), IndexError> {
        self.0
            .update(is_list, |row| {
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
                String::from_utf8(encode(&list))
                    .map(Some)
                    .map_err(|e| e.to_string())
            })
            .map(|_| ())
            .map_err(|why| {
                if why == UNREADABLE {
                    IndexError::Unreadable
                } else {
                    IndexError::Write(why)
                }
            })
    }

    /// Replaces the list.
    pub fn write(&self, list: &[VaultEntry]) -> Result<(), IndexError> {
        self.update(|_| Ok(list.to_vec()))
    }

    /// Something that differs whenever the list does, for a window to see that it changed: the
    /// document's text hashed into the time (the database's own file changes with every setting
    /// and draft, and its modified time lags a write sitting in the log) and its length. `None`
    /// when there is no list or it can't be read.
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

/// Why `entry` can't be saved into `list` in place of `old`, or `None`.
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

/// Entries that fit a URL, a host or words from a task, best first; every entry for an empty
/// query, none when nothing fits.
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
                // api.github.com and uploads.github.com share github.com.
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
    // Stable: equal scores keep the list's order.
    scored.sort_by_key(|s| std::cmp::Reverse(s.0));
    scored.into_iter().map(|(_, e)| e.clone()).collect()
}

/// The environment a command gets for these entries, and the strings to hide in its output
/// (value → `[secret:<name>]`).
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

/// `github (api.github.com)`: the name with its URL's host, so a list matches a task by site.
pub fn label(e: &VaultEntry) -> String {
    match e.host() {
        Some(host) => format!("{} ({host})", e.name),
        None => e.name.clone(),
    }
}

/// One entry as `secrets` prints it: every line of the note and of the how-to indented under the
/// name. `today` is a day number ([`expiry::today_local`]).
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

/// `text` split into lines, each indented; a blank line stays empty and blank lines at the end go.
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
