use super::ids::is_uuid;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COPIES: AtomicU64 = AtomicU64::new(0);

pub trait Steps: Send + Sync {
    fn exists(&self, conversation: &str) -> bool;
    fn count(&self, conversation: &str) -> Option<u64>;
}

pub struct SqliteSteps {
    conversations: PathBuf,
    scratch: PathBuf,
}

impl SqliteSteps {
    pub fn new(conversations: PathBuf, scratch: PathBuf) -> SqliteSteps {
        SqliteSteps {
            conversations,
            scratch,
        }
    }

    fn db(&self, conversation: &str) -> Option<PathBuf> {
        if !is_uuid(conversation) {
            return None;
        }
        let path = self.conversations.join(format!("{conversation}.db"));
        let meta = std::fs::symlink_metadata(&path).ok()?;
        meta.is_file().then_some(path)
    }
}

impl Steps for SqliteSteps {
    fn exists(&self, conversation: &str) -> bool {
        self.db(conversation).is_some()
    }

    fn count(&self, conversation: &str) -> Option<u64> {
        let db = self.db(conversation)?;
        let dir = self.scratch.join(format!(
            "steps-{}-{}",
            std::process::id(),
            COPIES.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).ok()?;
        let counted = copy_and_count(&db, &dir);
        let _ = std::fs::remove_dir_all(&dir);
        counted
    }
}

fn copy_and_count(db: &Path, dir: &Path) -> Option<u64> {
    let name = db.file_name()?.to_string_lossy().into_owned();
    let copy = dir.join(&name);
    std::fs::copy(db, &copy).ok()?;
    for suffix in ["-wal", "-shm"] {
        let side = db.with_file_name(format!("{name}{suffix}"));
        if side.is_file() {
            let _ = std::fs::copy(&side, dir.join(format!("{name}{suffix}")));
        }
    }
    let conn = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let n: i64 = conn
        .query_row("select count(*) from steps", [], |row| row.get(0))
        .ok()?;
    u64::try_from(n).ok()
}
