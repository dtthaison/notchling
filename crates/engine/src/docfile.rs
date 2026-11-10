//! A file Notchling alone used to rewrite whole, now a document in `notchling.db`
//! (`crate::store`, decision 0014): the saved accounts lists and the daemon's vault list.
//!
//! A [`DocFile`] is the document's name plus the file it used to be. Reads take the document and,
//! while the old file hasn't moved in, that file's text; they never make a database and never
//! move anything, so a read-only caller (a list read while a switch is off, a render over a
//! checked-in fixture folder) writes nothing. A change moves the old file in first, once (a row
//! already there wins; a link, an unreadable file or one `valid` rejects stays where it is), then
//! changes the row in one transaction, so two writers never lose each other's change.

use crate::store;
use std::path::{Path, PathBuf};

/// One document and the file it used to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocFile {
    /// Notchling's data folder, where `notchling.db` is.
    pub data_dir: PathBuf,
    /// The document's name in the database.
    pub name: &'static str,
    /// The file it was before the database: read while the document is missing, moved in on the
    /// first change.
    pub old_file: PathBuf,
    /// An old file that can't be read, or isn't valid, makes every change fail: it is the
    /// source, and nothing is written over it (the vault list's rule). Off, a change goes ahead
    /// and the bad file is ignored from then on, since the document is read first.
    pub strict: bool,
}

impl DocFile {
    pub fn new(data_dir: &Path, name: &'static str, old_file: PathBuf) -> DocFile {
        DocFile {
            data_dir: data_dir.to_path_buf(),
            name,
            old_file,
            strict: false,
        }
    }

    /// The same, refusing to write while the old file can't be read.
    pub fn strict(mut self) -> DocFile {
        self.strict = true;
        self
    }

    /// The text: the document, else the old file's (a link is refused, not followed). `Ok(None)`
    /// when there is neither; `Err` for a database that can't be opened or an old file that can't
    /// be read. Makes nothing.
    pub fn read(&self) -> Result<Option<String>, String> {
        if let Some(text) = store::doc(&self.data_dir, self.name)? {
            return Ok(Some(text));
        }
        match std::fs::symlink_metadata(&self.old_file) {
            Err(_) => Ok(None),
            Ok(meta) if !meta.is_file() => {
                Err(format!("{} is not a regular file", self.old_file.display()))
            }
            Ok(_) => std::fs::read_to_string(&self.old_file)
                .map(Some)
                .map_err(|e| e.to_string()),
        }
    }

    /// Moves the old file in (see the module doc). Best effort: a database failure shows again in
    /// the change that follows.
    pub fn import(&self, valid: impl Fn(&str) -> bool) {
        let _ = store::import_file(&self.data_dir, self.name, &self.old_file, valid);
    }

    /// Reads the document and writes what `change` makes of it, in one transaction. The old file
    /// is moved in first (`valid` decides whether it is a file of ours). `change` gets `None` when
    /// there is no document; returning `None` removes it, an error writes nothing. Returns what
    /// was written.
    pub fn update(
        &self,
        valid: impl Fn(&str) -> bool,
        change: impl FnOnce(Option<String>) -> Result<Option<String>, String>,
    ) -> Result<Option<String>, String> {
        self.import(valid);
        store::update_doc(&self.data_dir, self.name, |row| {
            if self.strict && row.is_none() && std::fs::symlink_metadata(&self.old_file).is_ok() {
                return Err(UNREADABLE.to_owned());
            }
            change(row)
        })
    }

    /// Replaces the document (a change that doesn't look at what was there).
    pub fn put(&self, valid: impl Fn(&str) -> bool, text: &str) -> Result<(), String> {
        self.update(valid, |_| Ok(Some(text.to_owned())))
            .map(|_| ())
    }
}

/// What [`DocFile::update`] fails with while a strict old file is still there.
pub const UNREADABLE: &str = "the old file can't be read, so it was left as it is";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::testutil::TempDir;

    fn is_object(text: &str) -> bool {
        text.trim_start().starts_with('{')
    }

    #[test]
    fn reads_make_nothing_and_fall_back_to_the_old_file() {
        let tmp = TempDir::new("docfile-read");
        let data = tmp.path("data");
        let old = tmp.write("data/daemon/thing.json", r#"{"a":1}"#);
        let file = DocFile::new(&data, "daemon/thing", old.clone());
        assert_eq!(file.read().unwrap().as_deref(), Some(r#"{"a":1}"#));
        assert!(!store::exists(&data), "a read makes no database");
        assert!(old.exists(), "and moves nothing");
        let none = DocFile::new(&data, "daemon/none", tmp.path("data/none.json"));
        assert_eq!(none.read().unwrap(), None);
    }

    #[test]
    fn the_first_change_moves_the_old_file_in_and_the_document_wins_after() {
        let tmp = TempDir::new("docfile-move");
        let data = tmp.path("data");
        let old = tmp.write("data/thing.json", r#"{"a":1}"#);
        let file = DocFile::new(&data, "thing", old.clone());
        file.update(is_object, |row| {
            assert_eq!(
                row.as_deref(),
                Some(r#"{"a":1}"#),
                "the change sees the old text"
            );
            Ok(Some(r#"{"a":2}"#.to_owned()))
        })
        .unwrap();
        assert!(!old.exists(), "moved in");
        assert_eq!(file.read().unwrap().as_deref(), Some(r#"{"a":2}"#));
        // a file that comes back later is neither read nor imported over the row
        tmp.write("data/thing.json", r#"{"a":3}"#);
        assert_eq!(file.read().unwrap().as_deref(), Some(r#"{"a":2}"#));
    }

    #[test]
    fn an_unreadable_old_file_stays_and_a_strict_one_blocks_changes() {
        let tmp = TempDir::new("docfile-strict");
        let data = tmp.path("data");
        let old = tmp.write("data/thing.json", "not json");
        let loose = DocFile::new(&data, "thing", old.clone());
        loose.put(is_object, r#"{"a":1}"#).unwrap();
        assert!(old.exists(), "a file that isn't ours is left alone");
        assert_eq!(loose.read().unwrap().as_deref(), Some(r#"{"a":1}"#));

        let data = tmp.path("data2");
        let old = tmp.write("data2/thing.json", "not json");
        let strict = DocFile::new(&data, "thing", old.clone()).strict();
        assert_eq!(strict.put(is_object, "{}"), Err(UNREADABLE.to_owned()));
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "not json");
        assert_eq!(store::doc(&data, "thing").unwrap(), None, "nothing written");
    }

    #[test]
    fn two_writers_at_once_lose_nothing() {
        let tmp = TempDir::new("docfile-writers");
        let data = tmp.path("data");
        tmp.write("data/n.json", "0");
        let file = DocFile::new(&data, "n", tmp.path("data/n.json"));
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let file = file.clone();
                scope.spawn(move || {
                    for _ in 0..10 {
                        file.update(
                            |t| t.parse::<i64>().is_ok(),
                            |row| {
                                let n: i64 = row.as_deref().unwrap_or("0").parse().unwrap();
                                Ok(Some((n + 1).to_string()))
                            },
                        )
                        .unwrap();
                    }
                });
            }
        });
        assert_eq!(file.read().unwrap().as_deref(), Some("40"));
        assert!(!tmp.path("data/n.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_where_the_old_file_was_is_not_read_or_moved() {
        let tmp = TempDir::new("docfile-link");
        let data = tmp.path("data");
        let target = tmp.write("target.json", r#"{"a":1}"#);
        std::fs::create_dir_all(&data).unwrap();
        let old = tmp.path("data/thing.json");
        std::os::unix::fs::symlink(&target, &old).unwrap();
        let file = DocFile::new(&data, "thing", old.clone());
        assert!(file.read().is_err());
        file.put(is_object, "{}").unwrap();
        assert!(
            old.exists() && target.exists(),
            "the link and its target are untouched"
        );
    }
}
