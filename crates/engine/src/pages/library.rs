//! The library on disk: `pages.json`, the page folders, finding and describing.

use super::clock;
use super::lock::{self, DirLock};
use crate::search::{find_pages, PageEntry};
use serde_json::Value;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Why a library write didn't happen.
#[derive(Debug)]
pub enum PagesError {
    /// The library was opened read-only: the daemon reads the Swift app's folder, it doesn't
    /// write it (decision 0011: one owner per resource).
    ReadOnly,
    /// Another writer held `.lock` for the whole wait.
    Busy,
    Io(io::Error),
}

impl fmt::Display for PagesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PagesError::ReadOnly => write!(f, "the pages library is read-only here"),
            PagesError::Busy => write!(f, "another process is writing the pages library"),
            PagesError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PagesError {}

impl From<io::Error> for PagesError {
    fn from(e: io::Error) -> Self {
        PagesError::Io(e)
    }
}

impl From<notchling_platform::Error> for PagesError {
    fn from(e: notchling_platform::Error) -> Self {
        PagesError::Io(io::Error::other(e.to_string()))
    }
}

/// A folder of saved pages. Open it read-only to look, writable to record.
#[derive(Clone, Debug)]
pub struct PageLibrary {
    dir: PathBuf,
    writable: bool,
}

/// Whether `part` is one plain path component: no separator, no `.` or `..`, no NUL. A folder or
/// file name read from `pages.json` is data, so it is checked before it becomes a path.
pub fn safe_component(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && !part.contains(['/', '\\', '\0'])
        && !part.contains(':')
}

/// A published path inside a page's folder: relative, with no empty, `.` or `..` part.
pub fn safe_relative(path: &str) -> Option<Vec<&str>> {
    if path.starts_with('/') || path.contains(['\\', '\0']) {
        return None;
    }
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    (!parts.is_empty()
        && parts
            .iter()
            .all(|p| *p != "." && *p != ".." && !p.contains(':')))
    .then_some(parts)
}

impl PageLibrary {
    /// Looks only. Every write answers [`PagesError::ReadOnly`].
    pub fn read_only(dir: impl Into<PathBuf>) -> Self {
        PageLibrary {
            dir: dir.into(),
            writable: false,
        }
    }

    /// May record and remove. For a folder this process owns, or a test's.
    pub fn writable(dir: impl Into<PathBuf>) -> Self {
        PageLibrary {
            dir: dir.into(),
            writable: true,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn is_writable(&self) -> bool {
        self.writable
    }

    pub fn list_path(&self) -> PathBuf {
        self.dir.join("pages.json")
    }

    pub fn index_path(&self) -> PathBuf {
        self.dir.join("index.html")
    }

    /// Every saved page, newest first. A missing or unreadable list is empty; an entry without
    /// `folder` and `file` is skipped.
    pub fn entries(&self) -> Vec<PageEntry> {
        let mut list = self.read_list();
        list.sort_by(|a, b| {
            b.updated
                .partial_cmp(&a.updated)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        list
    }

    /// The list as stored (file order).
    pub(crate) fn read_list(&self) -> Vec<PageEntry> {
        let Ok(data) = std::fs::read(self.list_path()) else {
            return Vec::new();
        };
        let Ok(root) = serde_json::from_slice::<Value>(&data) else {
            return Vec::new();
        };
        root.get("pages")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| serde_json::from_value::<PageEntry>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// When `pages.json` last changed (seconds since 1970), for callers that poll.
    pub fn list_modified(&self) -> Option<f64> {
        let modified = std::fs::metadata(self.list_path()).ok()?.modified().ok()?;
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs_f64())
    }

    /// The page with this link or id, else the pages whose text holds every word of the query.
    pub fn find(&self, query: &str) -> Vec<PageEntry> {
        find_in(query, &self.entries())
    }

    /// The page's file on disk, `None` when its stored folder or file name isn't a plain name (a
    /// hand-edited `pages.json` must not point a reader outside the library).
    pub fn local_path(&self, entry: &PageEntry) -> Option<PathBuf> {
        if !safe_component(&entry.folder) || !safe_component(&entry.file) {
            return None;
        }
        Some(self.dir.join(&entry.folder).join(&entry.file))
    }

    /// Plain lines an agent can act on: where the copy is, and who can republish it.
    pub fn describe(&self, entry: &PageEntry) -> String {
        let date = clock::local_text(entry.updated);
        let mut lines = vec![
            format!("Title: {}", entry.title),
            format!(
                "Link: {}",
                if entry.url.is_empty() {
                    "(never published to claude.ai)"
                } else {
                    &entry.url
                }
            ),
            format!(
                "Published by: {}",
                if entry.account.is_empty() {
                    "unknown account"
                } else {
                    &entry.account
                }
            ),
        ];
        if !entry.project.is_empty() {
            lines.push(format!("Project: {}", entry.project));
        }
        lines.push(format!("Last published: {date}"));
        match self.local_path(entry) {
            Some(path) => lines.push(format!("Local copy: {}", path.display())),
            None => lines.push("Local copy: (the library's entry is damaged)".to_string()),
        }
        let others = self.supporting_files(entry);
        if !others.is_empty() {
            lines.push(format!(
                "Supporting files (next to it): {}",
                others.join(", ")
            ));
        }
        if !entry.source.is_empty() && Path::new(&entry.source).exists() {
            lines.push(format!("Original source file: {}", entry.source));
        }
        lines.join("\n")
    }

    /// The files in the page's folder besides the page itself, sorted, as paths relative to it.
    /// Hidden files and links are left out.
    pub fn supporting_files(&self, entry: &PageEntry) -> Vec<String> {
        const MAX_FILES: usize = 1000;
        let Some(main) = self.local_path(entry) else {
            return Vec::new();
        };
        let Some(root) = main.parent() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut stack = vec![(root.to_path_buf(), String::new())];
        while let Some((dir, prefix)) = stack.pop() {
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            for item in read.flatten() {
                let name = item.file_name().to_string_lossy().into_owned();
                let rel = format!("{prefix}{name}");
                if rel.starts_with('.') || rel == entry.file {
                    continue;
                }
                match item.file_type() {
                    Ok(t) if t.is_dir() => stack.push((item.path(), format!("{rel}/"))),
                    Ok(t) if t.is_file() && out.len() < MAX_FILES => out.push(rel),
                    _ => {}
                }
            }
        }
        out.sort();
        out
    }

    /// Runs `body` holding the library's write lock. Fails closed: read-only, or busy.
    pub(crate) fn with_lock<T>(
        &self,
        body: impl FnOnce() -> Result<T, PagesError>,
    ) -> Result<T, PagesError> {
        if !self.writable {
            return Err(PagesError::ReadOnly);
        }
        notchling_platform::files::ensure_private_dir(&self.dir)?;
        let _held: DirLock = lock::acquire(&self.dir.join(".lock"))?.ok_or(PagesError::Busy)?;
        body()
    }

    /// Writes `pages.json` and the list page. Called with the lock held.
    pub(crate) fn save_list(&self, list: &[PageEntry]) -> Result<(), PagesError> {
        let value = serde_json::json!({ "pages": list });
        let data = serde_json::to_vec(&value).map_err(io::Error::other)?;
        notchling_platform::files::write_private_no_follow(&self.list_path(), &data)?;
        super::listpage::write(self, list)
    }

    /// Forgets a page and deletes its local folder. Never touches the page on claude.ai.
    pub fn remove(&self, folder: &str) -> Result<(), PagesError> {
        self.with_lock(|| {
            let list: Vec<PageEntry> = self
                .read_list()
                .into_iter()
                .filter(|e| e.folder != folder)
                .collect();
            if safe_component(folder) {
                let _ = std::fs::remove_dir_all(self.dir.join(folder));
            }
            self.save_list(&list)
        })
    }
}

/// [`PageLibrary::find`] over a list already read (the search panel searches on every key).
pub fn find_in(query: &str, all: &[PageEntry]) -> Vec<PageEntry> {
    find_pages(query, all).into_iter().cloned().collect()
}
