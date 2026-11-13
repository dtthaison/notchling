use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::SystemTime;

use notchling_engine::sessions::Agent;
use notchling_engine::tools::agy::summaries::{self, Row, DB_NAME};

use super::foreign::agy_agent;

const LIST_EVERY_MS: i64 = 3000;
const REUSE_MAX_MS: i64 = 5 * 60 * 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp(Option<(SystemTime, u64)>);

impl Stamp {
    fn of(path: &Path) -> Stamp {
        Stamp(
            std::fs::metadata(path)
                .ok()
                .and_then(|m| m.modified().ok().map(|t| (t, m.len()))),
        )
    }
}

#[derive(Debug, Default)]
struct State {
    listed_at_ms: Option<i64>,
    read_at_ms: i64,
    stamps: Option<[Stamp; 2]>,
    rows: Vec<Agent>,
}

#[derive(Clone, Debug)]
pub struct AgyList {
    pub root: PathBuf,
    pub copy_dir: PathBuf,
    state: Arc<Mutex<State>>,
}

type Lists = Mutex<HashMap<(PathBuf, PathBuf), AgyList>>;

impl AgyList {
    pub fn new(root: PathBuf, copy_dir: PathBuf) -> AgyList {
        AgyList {
            root,
            copy_dir,
            state: Arc::default(),
        }
    }

    pub fn shared(root: PathBuf, copy_dir: PathBuf) -> AgyList {
        static LISTS: OnceLock<Lists> = OnceLock::new();
        let mut lists = LISTS
            .get_or_init(Lists::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        lists
            .entry((root.clone(), copy_dir.clone()))
            .or_insert_with(|| AgyList::new(root, copy_dir))
            .clone()
    }

    #[cfg(test)]
    pub fn is_same_list(&self, other: &AgyList) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn rows(&self, now_ms: i64) -> Vec<Agent> {
        self.rows_with(now_ms, |root, copy_dir, now| {
            summaries::rows(root, copy_dir, now)
        })
    }

    fn rows_with(
        &self,
        now_ms: i64,
        read: impl FnOnce(&Path, &Path, f64) -> Vec<Row>,
    ) -> Vec<Agent> {
        let mut state = self.state();
        let stale = state
            .listed_at_ms
            .is_none_or(|at| now_ms - at >= LIST_EVERY_MS || now_ms < at);
        if !stale {
            return state.rows.clone();
        }
        state.listed_at_ms = Some(now_ms);
        let db = self.root.join(DB_NAME);
        let stamps = [Stamp::of(&db), Stamp::of(&with_wal(&db))];
        let unchanged = state.stamps.as_ref() == Some(&stamps)
            && now_ms >= state.read_at_ms
            && now_ms - state.read_at_ms < REUSE_MAX_MS;
        if !unchanged {
            let rows = read(&self.root, &self.copy_dir, now_ms as f64 / 1000.0);
            let failed = rows.is_empty() && stamps[0].0.is_some();
            state.rows = rows.iter().map(agy_agent).collect();
            state.stamps = (!failed).then_some(stamps);
            state.read_at_ms = now_ms;
        }
        state.rows.clone()
    }
}

fn with_wal(db: &Path) -> PathBuf {
    let mut name = db.as_os_str().to_os_string();
    name.push("-wal");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn row(id: &str) -> Row {
        Row {
            id: id.into(),
            title: format!("T {id}"),
            folder: "/w".into(),
            modified: 1_000.0,
            running: false,
        }
    }

    fn rig(name: &str) -> (crate::testutil::Scratch, AgyList) {
        let scratch = crate::testutil::Scratch::new(name);
        let root = scratch.path().join("antigravity-cli");
        std::fs::create_dir_all(&root).expect("a folder");
        std::fs::write(root.join(DB_NAME), "db").expect("a file");
        std::fs::write(root.join(format!("{DB_NAME}-wal")), "wal").expect("a file");
        let list = AgyList::new(root, scratch.path().join("copy"));
        (scratch, list)
    }

    #[test]
    fn the_list_is_empty_without_a_database_and_looks_every_few_seconds() {
        let scratch = crate::testutil::Scratch::new("agy-list");
        let list = AgyList::new(
            scratch.path().join("antigravity-cli"),
            scratch.path().join("copy"),
        );
        assert!(list.rows(1_000).is_empty());
        assert_eq!(list.state().listed_at_ms, Some(1_000));
        list.rows(2_000);
        assert_eq!(list.state().listed_at_ms, Some(1_000), "not yet stale");
        list.rows(4_000);
        assert_eq!(list.state().listed_at_ms, Some(4_000));
        assert!(
            !scratch.path().join("copy/summaries.db").exists(),
            "nothing to copy"
        );
    }

    #[test]
    fn files_that_did_not_change_are_not_read_again() {
        let (_scratch, list) = rig("agy-list-stamp");
        let reads = Cell::new(0);
        let read = |_: &Path, _: &Path, _: f64| {
            reads.set(reads.get() + 1);
            vec![row("a")]
        };
        assert_eq!(list.rows_with(1_000, read).len(), 1);
        assert_eq!(reads.get(), 1);
        assert_eq!(list.rows_with(5_000, read).len(), 1, "the rows are kept");
        assert_eq!(list.rows_with(9_000, read).len(), 1);
        assert_eq!(reads.get(), 1);
        let wal = list.root.join(format!("{DB_NAME}-wal"));
        std::fs::write(&wal, "wal, longer").expect("a write");
        assert_eq!(list.rows_with(13_000, read).len(), 1);
        assert_eq!(reads.get(), 2);
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(list.root.join(DB_NAME), "DB").expect("a write");
        list.rows_with(17_000, read);
        assert_eq!(reads.get(), 3);
        list.rows_with(17_000 + REUSE_MAX_MS, read);
        assert_eq!(reads.get(), 4);
    }

    #[test]
    fn a_read_that_found_nothing_in_a_database_that_is_there_is_tried_again() {
        let (_scratch, list) = rig("agy-list-failed");
        let reads = Cell::new(0);
        let nothing = |_: &Path, _: &Path, _: f64| {
            reads.set(reads.get() + 1);
            Vec::new()
        };
        list.rows_with(1_000, nothing);
        list.rows_with(5_000, nothing);
        list.rows_with(9_000, nothing);
        assert_eq!(
            reads.get(),
            3,
            "no stamp is kept for a read that may have failed"
        );
    }

    #[test]
    fn a_missing_database_is_not_read_every_time_either() {
        let scratch = crate::testutil::Scratch::new("agy-list-none");
        let list = AgyList::new(scratch.path().join("none"), scratch.path().join("copy"));
        let reads = Cell::new(0);
        let read = |_: &Path, _: &Path, _: f64| {
            reads.set(reads.get() + 1);
            Vec::new()
        };
        list.rows_with(1_000, read);
        list.rows_with(5_000, read);
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn callers_asking_for_the_same_folders_share_one_list() {
        let scratch = crate::testutil::Scratch::new("agy-list-shared");
        let (root, copy) = (scratch.path().join("a"), scratch.path().join("copy"));
        let (notch, chat) = (
            AgyList::shared(root.clone(), copy.clone()),
            AgyList::shared(root.clone(), copy.clone()),
        );
        let reads = Cell::new(0);
        let read = |_: &Path, _: &Path, _: f64| {
            reads.set(reads.get() + 1);
            vec![row("a")]
        };
        assert_eq!(notch.rows_with(1_000, read).len(), 1);
        assert_eq!(chat.rows_with(1_500, read).len(), 1);
        assert_eq!(reads.get(), 1);
        let other = AgyList::shared(root, scratch.path().join("copy2"));
        other.rows_with(1_600, read);
        assert_eq!(reads.get(), 2);
        let alone = AgyList::new(scratch.path().join("a"), copy);
        alone.rows_with(1_700, read);
        assert_eq!(reads.get(), 3);
    }
}
